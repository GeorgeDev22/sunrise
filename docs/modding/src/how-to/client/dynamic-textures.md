# Textures made at run time

A client mod that bakes its own images (a panel, a gauge, glyphs from its own font, a map, a schematic view) creates
each one under a name and draws it as a GUI sprite. This is vanilla's `DynamicTexture`: Java registers one with
`TextureManager.register(id, texture)`, sends its pixels with `DynamicTexture.upload()`, draws it with
`GuiGraphics.blit(id, ...)` and frees it with `TextureManager.release(id)`. In Sunrise the image lives in a slot of
the GUI atlas, so it draws as any other GUI sprite.

```rust
use mod_api::prelude::*;
use mod_api::{DynamicTextureError, Identifier};

const PANEL: &str = "mymod:dynamic/panel";

/// Bake the panel once: 64 by 32 pixels, row by row from the top left, four bytes (RGBA) each.
fn bake_panel(host: &mut dyn ClientHost) -> Result<(), DynamicTextureError> {
    let (width, height) = (64, 32);
    let rgba = [24, 24, 48, 230].repeat(width * height);
    let id = Identifier::parse(PANEL).unwrap();
    host.create_dynamic_texture(&id, width as u32, height as u32, &rgba)
}

fn gui_render_post(&self, _: &dyn ClientHost, event: &mut GuiRenderPostEvent<'_>) {
    // The whole image (`src` in its own pixels), stretched over 128 by 64 GUI pixels.
    event
        .batch
        .sprite(PANEL, [0.0, 0.0, 64.0, 32.0], [8.0, 8.0, 128.0, 64.0], None);
}
```

The four calls are on the client host, the same for native and wasm mods (the pixels cross as bytes). They change
the game's atlas, so they take `&mut dyn ClientHost`: call them from a hook that has it, such as `client_tick_post`,
not from inside a draw.

- **`create_dynamic_texture(id, width, height, rgba)`** makes the image. From the next frame, its id is a GUI sprite of
  `width` x `height` pixels: `DrawContext::sprite`, `gui_nine_slice`, `tiled_sprite`, `gui_paint`'s `Blit` and
  `gui_sprite` draw and find it as any other sprite.
- **`update_dynamic_texture(id, width, height, rgba)`** gives it new pixels, written into the slot it already holds on
  the next frame. The size may change within the image's class (below); the sprite takes the new size. Update when
  the image changed, not every frame: each update copies the pixels and uploads the image.
- **`free_dynamic_texture(id)`** frees it: the name is gone at once, the slot goes back to the pool and the image's
  share of the budget back to the mod. An image never freed lasts until the game ends.
- **`dynamic_texture_budget()`** answers what the mod may still create, `DynamicTextureBudget { images, bytes }`.

## Names

The id is in the calling mod's namespace (`mymod:...`); the path is the mod's to choose. No mod can update or free
another's image: an id in another namespace is `NotOwned`. An id that already names one of the mod's images, or a GUI
sprite of the packs (`mymod:gui/panel`), is `NameTaken`.

## Slots and size classes

The images live in a pool of fixed slots of the GUI atlas, in three size classes (`DYNAMIC_TEXTURE_CLASSES`). An image
takes a slot of the smallest class whose side holds both its width and its height:

| Class | Slot | Slots in the pool |
|---|---|---|
| Small | 64 x 64 | 32 |
| Medium | 256 x 256 | 8 |
| Large | 1024 x 1024 | 2 |

The slots are shared by every mod, first come. When every slot of the image's class is held, `create` is `NoSlot`:
free an image, or make it fit a smaller class. A side longer than 1024 (`DYNAMIC_TEXTURE_MAX_SIDE`) is `TooLarge`:
textures of their own, for larger images, are not part of this API.

An update re-patches the slot the image holds, so a size of another class is `ClassChange`: free the image and create
it again at the new size.

The pool exists only when a client mod is loaded, native or wasm: a game without mods has no slot and its atlas does
not grow. Where there is no pool (a server, or no client mod loaded) every call is `Unavailable`.

## Budgets

| Limit | Value | Refusal |
|---|---|---|
| Images a mod holds | `DYNAMIC_TEXTURE_MOD_IMAGES`, 16 | `OverBudget` |
| Pixels a mod holds | `DYNAMIC_TEXTURE_MOD_BYTES`, 4 MiB (`width * height * 4` an image) | `OverBudget` |
| Longest side | `DYNAMIC_TEXTURE_MAX_SIDE`, 1024 pixels | `TooLarge` |

These are limits of the atlas route; vanilla's `TextureManager` has none, so read what remains with
`dynamic_texture_budget` rather than assume. A 1024 x 1024 image takes a mod's whole byte budget. The bytes are what
the game keeps of each image's last pixels, to put them back after a resource reload; an update counts its new size
in place of the old, and a free gives its share back.

## Refusals

Every call answers `Result<_, DynamicTextureError>`, and a refused call changes nothing:

| Refusal | When |
|---|---|
| `NotOwned` | the id is outside the calling mod's namespace |
| `BadSize` | a side is 0, or the pixels are not `width * height * 4` bytes |
| `TooLarge` | a side is longer than 1024 |
| `NoSlot` | every slot of the image's class is held |
| `OverBudget` | the mod would pass 16 images or 4 MiB |
| `NameTaken` | `create` of an id that names an image or a GUI sprite |
| `Unknown` | `update` or `free` of an id that names no image |
| `ClassChange` | `update` to a size of another class |
| `Unavailable` | no pool here: a server, or no client mod loaded |

Match `DynamicTextureError` non-exhaustively.

## Resource reloads

A resource pack change rebuilds the GUI atlas. Every image keeps its name, class and slot, and the game puts its last
pixels into the new atlas by itself: there is nothing to upload again. Sampling is the atlas's: nearest, as GUI
sprites.
