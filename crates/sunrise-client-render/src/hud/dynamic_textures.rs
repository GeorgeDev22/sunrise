//! Client mods' dynamic textures (`ClientRenderClientHost::create_dynamic_texture`, #1343),
//! vanilla's `DynamicTexture` registered in the `TextureManager` under an id. Here each image
//! lives in the HUD atlas, in a pool of fixed slots of three size classes
//! (`DYNAMIC_TEXTURE_CLASSES`, [`DYNAMIC_TEXTURE_SLOTS`] of each) that the atlas packs only when a
//! client mod is loaded ([`super::HudAtlas::build_over_with_dynamic_textures`]). An image takes a
//! slot of the smallest class that holds it, and its id names its part of the slot as a GUI
//! sprite, so every sprite path draws it as any other. An update re-patches the same slot; a
//! free drops the name and gives the slot back. The last pixels of each image stay here, counted
//! in the mod's byte budget, to patch a rebuilt atlas again after a resource reload.
use super::Rect;
use mod_api::{
    DYNAMIC_TEXTURE_CLASSES, DYNAMIC_TEXTURE_MAX_SIDE, DYNAMIC_TEXTURE_MOD_BYTES,
    DYNAMIC_TEXTURE_MOD_IMAGES, DynamicTextureBudget, DynamicTextureError,
};
use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};
use sunrise_core::identifier::Identifier;

/// The slots of each size class, in the order of `DYNAMIC_TEXTURE_CLASSES` (64, 256 and 1024
/// pixels square): 32, 8 and 2. The pool takes 32 x 64 x 64 + 8 x 256 x 256 + 2 x 1024 x 1024
/// texels, 10.5 MiB of the atlas, and only when a client mod is loaded. Two large slots let two
/// mods each show an image of their whole byte budget (a 1024 x 1024 map) where the single
/// shared slot before let one; eight medium and 32 small slots hold two mods' full count of
/// panels and icons. Each more large slot would cost the atlas 4 MiB.
pub const DYNAMIC_TEXTURE_SLOTS: [usize; 3] = [32, 8, 2];

const _: () = assert!(DYNAMIC_TEXTURE_CLASSES[2] == DYNAMIC_TEXTURE_MAX_SIDE);

/// The atlas sprite of slot `slot` of class `class` while the atlas is packed; the atlas then
/// takes these names out of its sprites ([`Pool::take`]), so no mod draws a slot by them.
pub(super) fn slot_sprite(class: usize, slot: usize) -> String {
    format!(
        "sunrise/dynamic-texture/{}/{slot}",
        DYNAMIC_TEXTURE_CLASSES[class]
    )
}

/// Where the pool's slots lie in one packed atlas.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pool {
    /// Each class's slots' top left corners, in texels.
    origins: [Vec<[u32; 2]>; 3],
    /// The atlas's width and height in texels.
    atlas: [u32; 2],
}

impl Pool {
    /// The packed slots of an atlas `width` x `height`, taken out of its `sprites`; none when it
    /// packed none (no client mod was loaded).
    pub(super) fn take(
        sprites: &mut HashMap<String, Rect>,
        width: u32,
        height: u32,
    ) -> Option<Self> {
        let mut origins: [Vec<[u32; 2]>; 3] = Default::default();
        for (class, origins) in origins.iter_mut().enumerate() {
            for slot in 0..DYNAMIC_TEXTURE_SLOTS[class] {
                let rect = sprites.remove(&slot_sprite(class, slot))?;
                origins.push([
                    (rect.u0 * width as f32).round() as u32,
                    (rect.v0 * height as f32).round() as u32,
                ]);
            }
        }
        Some(Self {
            origins,
            atlas: [width, height],
        })
    }
    /// The slots of each class, as [`DYNAMIC_TEXTURE_SLOTS`] has them.
    pub fn slots(&self) -> [usize; 3] {
        self.origins.each_ref().map(Vec::len)
    }
    /// The top left corner of slot `slot` of class `class`, in texels.
    pub fn origin(&self, class: usize, slot: usize) -> [u32; 2] {
        self.origins[class][slot]
    }
    /// The sprite of a `size` image in slot `slot` of class `class`: the image's part of the
    /// slot, its GUI size the image's.
    fn rect(&self, class: usize, slot: usize, size: [u32; 2]) -> Rect {
        let [x, y] = self.origin(class, slot);
        let [width, height] = self.atlas.map(|side| side as f32);
        Rect {
            u0: x as f32 / width,
            v0: y as f32 / height,
            u1: (x + size[0]) as f32 / width,
            v1: (y + size[1]) as f32 / height,
            w: size[0],
            h: size[1],
        }
    }
}

/// An image's pixels to write into the atlas before the next draw: the whole image, at its
/// slot's corner. The pixels are the kept copy itself, shared rather than copied again.
#[derive(Clone, Debug)]
pub struct ImagePatch {
    pub origin: [u32; 2],
    pub size: [u32; 2],
    pub rgba: Arc<[u8]>,
}

/// Every mod's dynamic textures: which slot each holds, and its last pixels.
#[derive(Default)]
pub struct DynamicTextures {
    /// Each image by its id; its owner is the id's namespace.
    images: BTreeMap<Identifier, Image>,
    limits: Limits,
}

struct Image {
    /// Its class, an index into `DYNAMIC_TEXTURE_CLASSES`, and its slot in that class.
    class: usize,
    slot: usize,
    size: [u32; 2],
    /// The last pixels given, `size` RGBA: what the atlas gets again after a rebuild.
    pixels: Arc<[u8]>,
    /// Whether the atlas lacks these pixels yet: set by a create, an update or a rebuilt atlas,
    /// cleared when [`DynamicTextures::patches`] hands them to a frame.
    dirty: bool,
}

/// The budgets of each mod and the slots of each class: the API's own, smaller in tests.
struct Limits {
    images: u32,
    bytes: u64,
    slots: [usize; 3],
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            images: DYNAMIC_TEXTURE_MOD_IMAGES,
            bytes: DYNAMIC_TEXTURE_MOD_BYTES,
            slots: DYNAMIC_TEXTURE_SLOTS,
        }
    }
}

impl DynamicTextures {
    /// Mod `owner` (none outside a mod's call) creates image `id` of `size` (width, height) from
    /// `rgba`, `width * height` pixels of four bytes, in a free slot of `pool` (none when the atlas
    /// has no pool), and names it among the atlas's `sprites`. Every refusal comes before the
    /// pixels are copied, and changes nothing.
    pub fn create(
        &mut self,
        pool: Option<&Pool>,
        sprites: &mut HashMap<String, Rect>,
        owner: Option<&str>,
        id: &Identifier,
        size: [u32; 2],
        rgba: &[u8],
    ) -> Result<(), DynamicTextureError> {
        let pool = pool.ok_or(DynamicTextureError::Unavailable)?;
        let owner = owned(owner, id)?;
        let (class, bytes) = sized(size, rgba.len())?;
        if self.images.contains_key(id) {
            return Err(DynamicTextureError::NameTaken);
        }
        let name = id.to_string();
        if sprites.contains_key(&name) {
            return Err(DynamicTextureError::NameTaken);
        }
        let held = self.held(owner);
        if held.images + 1 > self.limits.images || held.bytes + bytes > self.limits.bytes {
            return Err(DynamicTextureError::OverBudget);
        }
        let slots = self.limits.slots[class].min(pool.slots()[class]);
        let slot = (0..slots)
            .find(|slot| {
                !self
                    .images
                    .values()
                    .any(|image| image.class == class && image.slot == *slot)
            })
            .ok_or(DynamicTextureError::NoSlot)?;
        sprites.insert(name, pool.rect(class, slot, size));
        self.images.insert(
            id.clone(),
            Image {
                class,
                slot,
                size,
                pixels: rgba.into(),
                dirty: true,
            },
        );
        Ok(())
    }
    /// Mod `owner` gives its image `id` new pixels, of a `size` of the same class: they go into
    /// the slot it holds. The kept copy is overwritten in place when its size is unchanged and no
    /// frame still holds it. Every refusal comes before any copy, and changes nothing.
    pub fn update(
        &mut self,
        pool: Option<&Pool>,
        sprites: &mut HashMap<String, Rect>,
        owner: Option<&str>,
        id: &Identifier,
        size: [u32; 2],
        rgba: &[u8],
    ) -> Result<(), DynamicTextureError> {
        let pool = pool.ok_or(DynamicTextureError::Unavailable)?;
        let owner = owned(owner, id)?;
        let held = self.held(owner);
        let image = self
            .images
            .get_mut(id)
            .ok_or(DynamicTextureError::Unknown)?;
        let (class, bytes) = sized(size, rgba.len())?;
        if class != image.class {
            return Err(DynamicTextureError::ClassChange);
        }
        if held.bytes - image.pixels.len() as u64 + bytes > self.limits.bytes {
            return Err(DynamicTextureError::OverBudget);
        }
        match Arc::get_mut(&mut image.pixels) {
            Some(pixels) if pixels.len() == rgba.len() => pixels.copy_from_slice(rgba),
            _ => image.pixels = rgba.into(),
        }
        if image.size != size {
            image.size = size;
            sprites.insert(id.to_string(), pool.rect(class, image.slot, size));
        }
        image.dirty = true;
        Ok(())
    }
    /// Mod `owner` frees its image `id`: the name leaves the atlas's `sprites`, the slot and the
    /// budget come back. A refusal changes nothing.
    pub fn free(
        &mut self,
        pool: Option<&Pool>,
        sprites: &mut HashMap<String, Rect>,
        owner: Option<&str>,
        id: &Identifier,
    ) -> Result<(), DynamicTextureError> {
        pool.ok_or(DynamicTextureError::Unavailable)?;
        owned(owner, id)?;
        self.images.remove(id).ok_or(DynamicTextureError::Unknown)?;
        sprites.remove(&id.to_string());
        Ok(())
    }
    /// What mod `owner` may still create.
    pub fn budget(
        &self,
        pool: Option<&Pool>,
        owner: Option<&str>,
    ) -> Result<DynamicTextureBudget, DynamicTextureError> {
        pool.ok_or(DynamicTextureError::Unavailable)?;
        let owner = owner.ok_or(DynamicTextureError::NotOwned)?;
        let held = self.held(owner);
        Ok(mod_api::__private::built::dynamic_texture_budget(
            self.limits.images - held.images,
            self.limits.bytes - held.bytes,
        ))
    }
    /// A rebuilt atlas (a resource reload) with `pool` and `sprites`: every image keeps its name,
    /// class and slot there and is patched again with its kept pixels. An atlas with no pool
    /// leaves the names unbound and the pixels kept, until one has a pool again.
    pub fn rebind(&mut self, pool: Option<&Pool>, sprites: &mut HashMap<String, Rect>) {
        let Some(pool) = pool else { return };
        for (id, image) in &mut self.images {
            sprites.insert(
                id.to_string(),
                pool.rect(image.class, image.slot, image.size),
            );
            image.dirty = true;
        }
    }
    /// A new renderer, whose atlas texture holds only the packed pixels: every image is patched
    /// again.
    pub fn forget_uploads(&mut self) {
        for image in self.images.values_mut() {
            image.dirty = true;
        }
    }
    /// The images the atlas texture lacks, as patches at their slots in `pool`, each once until it
    /// changes again. They share the kept pixels.
    pub fn patches(&mut self, pool: Option<&Pool>, patches: &mut Vec<ImagePatch>) {
        let Some(pool) = pool else { return };
        for image in self.images.values_mut().filter(|image| image.dirty) {
            image.dirty = false;
            patches.push(ImagePatch {
                origin: pool.origin(image.class, image.slot),
                size: image.size,
                rgba: image.pixels.clone(),
            });
        }
    }
    /// The images and bytes mod `owner` holds.
    fn held(&self, owner: &str) -> Held {
        self.images
            .iter()
            .filter(|(id, _)| id.namespace() == owner)
            .fold(Held::default(), |held, (_, image)| Held {
                images: held.images + 1,
                bytes: held.bytes + image.pixels.len() as u64,
            })
    }
}

#[derive(Clone, Copy, Default)]
struct Held {
    images: u32,
    bytes: u64,
}

/// `owner`, when `id` is in its namespace.
fn owned<'a>(owner: Option<&'a str>, id: &Identifier) -> Result<&'a str, DynamicTextureError> {
    owner
        .filter(|owner| *owner == id.namespace())
        .ok_or(DynamicTextureError::NotOwned)
}

/// The class a `[width, height]` image of `len` bytes takes, and its bytes: `BadSize` for a side
/// of 0 or pixels that are not `width * height * 4` bytes, `TooLarge` past the largest class.
/// The class is found before the bytes are counted, so the count only ever sees sides of at most
/// the largest class and cannot overflow.
fn sized([width, height]: [u32; 2], len: usize) -> Result<(usize, u64), DynamicTextureError> {
    if width == 0 || height == 0 {
        return Err(DynamicTextureError::BadSize);
    }
    let class = DYNAMIC_TEXTURE_CLASSES
        .iter()
        .position(|side| width <= *side && height <= *side)
        .ok_or(DynamicTextureError::TooLarge)?;
    let bytes = u64::from(width) * u64::from(height) * 4;
    if bytes != len as u64 {
        return Err(DynamicTextureError::BadSize);
    }
    Ok((class, bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use DynamicTextureError::*;
    use mod_api::__private::built::dynamic_texture_budget;

    fn id(text: &str) -> Identifier {
        Identifier::parse(text).unwrap()
    }

    /// A pool of two small slots, one medium and one large, in a 2048 x 2048 atlas.
    fn pool() -> Pool {
        Pool {
            origins: [vec![[0, 0], [64, 0]], vec![[0, 64]], vec![[1024, 0]]],
            atlas: [2048, 2048],
        }
    }

    /// Three images and 64 x 64 x 4 + 16 bytes a mod; the slots of [`pool`].
    fn textures() -> DynamicTextures {
        DynamicTextures {
            images: BTreeMap::new(),
            limits: Limits {
                images: 3,
                bytes: 64 * 64 * 4 + 16,
                slots: [2, 1, 1],
            },
        }
    }

    /// The ids, classes, slots, sizes and pixels held, and the atlas's sprite names.
    type State = (Vec<(String, usize, usize, [u32; 2], Vec<u8>)>, Vec<String>);
    fn state(textures: &DynamicTextures, sprites: &HashMap<String, Rect>) -> State {
        let images = textures
            .images
            .iter()
            .map(|(id, image)| {
                let pixels = image.pixels.to_vec();
                (id.to_string(), image.class, image.slot, image.size, pixels)
            })
            .collect();
        let mut names: Vec<_> = sprites.keys().cloned().collect();
        names.sort();
        (images, names)
    }

    /// Where sprite `name` lies, in texels of the 2048 x 2048 atlas, and its GUI size.
    fn texels(sprites: &HashMap<String, Rect>, name: &str) -> [u32; 6] {
        let rect = sprites[name];
        let [u0, v0, u1, v1] = [rect.u0, rect.v0, rect.u1, rect.v1].map(|uv| (uv * 2048.0) as u32);
        [u0, v0, u1, v1, rect.w, rect.h]
    }

    #[test]
    fn an_image_takes_the_smallest_class_that_holds_it() {
        let class = |width: u32, height: u32| {
            sized([width, height], (width * height * 4) as usize).map(|(class, _)| class)
        };
        assert_eq!(class(1, 1), Ok(0));
        assert_eq!(class(64, 64), Ok(0));
        assert_eq!(class(65, 1), Ok(1));
        assert_eq!(class(1, 256), Ok(1));
        assert_eq!(class(257, 2), Ok(2));
        assert_eq!(class(1024, 1024), Ok(2));
        assert_eq!(class(1025, 1), Err(TooLarge));
        assert_eq!(class(0, 4), Err(BadSize));
        assert_eq!(sized([2, 2], 15), Err(BadSize));
        assert_eq!(sized([2, 2], 17), Err(BadSize));
        // Extreme sides are refused by their class, before any byte count.
        assert_eq!(sized([u32::MAX, u32::MAX], 0), Err(TooLarge));
        assert_eq!(sized([u32::MAX, 1], 0), Err(TooLarge));
        assert_eq!(sized([1, u32::MAX], 4), Err(TooLarge));
        assert_eq!(sized([0, u32::MAX], 0), Err(BadSize));
        assert_eq!(sized([1025, 1025], 0), Err(TooLarge));
    }

    /// The slot and budget decisions, with no GPU: a full class has no slot for any mod, a freed
    /// slot and budget come back, the budget counts images and the kept pixels, and an update
    /// writes into the slot the image holds.
    #[test]
    fn slots_and_budgets_decide_without_a_gpu() {
        fn create(
            textures: &mut DynamicTextures,
            sprites: &mut HashMap<String, Rect>,
            owner: &str,
            name: &str,
            size: [u32; 2],
        ) -> Result<(), DynamicTextureError> {
            let rgba = vec![7; (size[0] * size[1] * 4) as usize];
            textures.create(Some(&pool()), sprites, Some(owner), &id(name), size, &rgba)
        }
        let pool = pool();
        let (mut textures, mut sprites) = (textures(), HashMap::new());
        assert_eq!(
            create(&mut textures, &mut sprites, "alpha", "alpha:a", [2, 1]),
            Ok(())
        );
        assert_eq!(
            create(&mut textures, &mut sprites, "alpha", "alpha:b", [64, 64]),
            Ok(())
        );
        // Both small slots are held: another mod's small image has none.
        assert_eq!(
            create(&mut textures, &mut sprites, "beta", "beta:a", [1, 1]),
            Err(NoSlot)
        );
        // alpha's bytes: 8 + 16384 of 16400; 260 more pass them, before the free medium slot.
        assert_eq!(
            create(&mut textures, &mut sprites, "alpha", "alpha:c", [65, 1]),
            Err(OverBudget)
        );
        let budget = textures.budget(Some(&pool), Some("alpha"));
        assert_eq!(budget, Ok(dynamic_texture_budget(1, 8)));
        assert_eq!(
            textures.budget(Some(&pool), Some("beta")),
            Ok(dynamic_texture_budget(3, 16400))
        );
        // A free gives the slot back to any mod, and the budget back to its owner.
        let b = id("alpha:b");
        assert_eq!(
            textures.free(Some(&pool), &mut sprites, Some("alpha"), &b),
            Ok(())
        );
        assert!(!sprites.contains_key("alpha:b"));
        assert_eq!(
            create(&mut textures, &mut sprites, "beta", "beta:a", [1, 1]),
            Ok(())
        );
        assert_eq!(textures.images[&id("beta:a")].slot, 1, "alpha:b's slot");
        assert_eq!(
            create(&mut textures, &mut sprites, "alpha", "alpha:c", [65, 1]),
            Ok(())
        );
        assert_eq!(
            create(&mut textures, &mut sprites, "alpha", "alpha:d", [300, 1]),
            Ok(())
        );
        // Three images: a fourth passes the count, whatever its size.
        assert_eq!(
            create(&mut textures, &mut sprites, "alpha", "alpha:e", [1, 1]),
            Err(OverBudget)
        );
        // Each image is named by its part of its slot, its GUI size its own.
        assert_eq!(texels(&sprites, "alpha:a"), [0, 0, 2, 1, 2, 1]);
        assert_eq!(texels(&sprites, "beta:a"), [64, 0, 65, 1, 1, 1]);
        assert_eq!(texels(&sprites, "alpha:c"), [0, 64, 65, 65, 65, 1]);
        assert_eq!(texels(&sprites, "alpha:d"), [1024, 0, 1324, 1, 300, 1]);

        // Each new image is patched once, at its slot.
        let mut patches = Vec::new();
        textures.patches(Some(&pool), &mut patches);
        let mut placed: Vec<_> = patches.iter().map(|p| (p.origin, p.size)).collect();
        placed.sort();
        let expected = [
            ([0, 0], [2, 1]),
            ([0, 64], [65, 1]),
            ([64, 0], [1, 1]),
            ([1024, 0], [300, 1]),
        ];
        assert_eq!(placed, expected);
        drop(patches);
        let mut patches = Vec::new();
        textures.patches(Some(&pool), &mut patches);
        assert!(patches.is_empty(), "nothing changed since");

        // An update writes the same slot: in place at the same size, the sprite resized within
        // its class, and the budget counts the new size.
        let a = id("alpha:a");
        let kept = Arc::as_ptr(&textures.images[&a].pixels);
        let rgba = [1, 2, 3, 4, 5, 6, 7, 8];
        let update = textures.update(Some(&pool), &mut sprites, Some("alpha"), &a, [2, 1], &rgba);
        assert_eq!(update, Ok(()));
        assert_eq!(
            Arc::as_ptr(&textures.images[&a].pixels),
            kept,
            "no new copy"
        );
        let c = id("alpha:c");
        let rgba = vec![9; 100 * 2 * 4];
        let update = textures.update(
            Some(&pool),
            &mut sprites,
            Some("alpha"),
            &c,
            [100, 2],
            &rgba,
        );
        assert_eq!(update, Ok(()));
        assert_eq!(texels(&sprites, "alpha:c"), [0, 64, 100, 66, 100, 2]);
        let mut patches = Vec::new();
        textures.patches(Some(&pool), &mut patches);
        let placed: Vec<_> = patches
            .iter()
            .map(|p| (p.origin, p.size, p.rgba[0]))
            .collect();
        assert_eq!(placed, [([0, 0], [2, 1], 1), ([0, 64], [100, 2], 9)]);
        let budget = textures.budget(Some(&pool), Some("alpha"));
        let bytes = 16400 - 8 - 800 - 1200;
        assert_eq!(budget, Ok(dynamic_texture_budget(0, bytes)));
        // Past the bytes, an update is refused too: 8 + 800 + 400 x 10 x 4 is more than 16400.
        let big = vec![0; 400 * 10 * 4];
        let d = id("alpha:d");
        let update = textures.update(
            Some(&pool),
            &mut sprites,
            Some("alpha"),
            &d,
            [400, 10],
            &big,
        );
        assert_eq!(update, Err(OverBudget));
    }

    /// Each refusal is named and leaves the images, their pixels and the atlas's names as they
    /// were.
    #[test]
    fn every_refusal_is_named_and_changes_nothing() {
        let pool = pool();
        let mut sprites = HashMap::new();
        let gui = Rect {
            u0: 0.0,
            v0: 0.0,
            u1: 0.0,
            v1: 0.0,
            w: 1,
            h: 1,
        };
        sprites.insert("alpha:gui/panel".to_owned(), gui);
        let mut textures = textures();
        let a = id("alpha:a");
        let created = textures.create(
            Some(&pool),
            &mut sprites,
            Some("alpha"),
            &a,
            [2, 1],
            &[5; 8],
        );
        assert_eq!(created, Ok(()));
        let before = state(&textures, &sprites);
        let none: Option<&Pool> = None;
        let refusals: [(&str, Result<(), DynamicTextureError>, DynamicTextureError); 21] = [
            (
                "create, no pool",
                textures.create(
                    none,
                    &mut sprites,
                    Some("alpha"),
                    &id("alpha:b"),
                    [1, 1],
                    &[0; 4],
                ),
                Unavailable,
            ),
            (
                "update, no pool",
                textures.update(none, &mut sprites, Some("alpha"), &a, [2, 1], &[0; 8]),
                Unavailable,
            ),
            (
                "free, no pool",
                textures.free(none, &mut sprites, Some("alpha"), &a),
                Unavailable,
            ),
            (
                "create in another namespace",
                textures.create(
                    Some(&pool),
                    &mut sprites,
                    Some("beta"),
                    &id("alpha:b"),
                    [1, 1],
                    &[0; 4],
                ),
                NotOwned,
            ),
            (
                "create outside a mod",
                textures.create(
                    Some(&pool),
                    &mut sprites,
                    None,
                    &id("alpha:b"),
                    [1, 1],
                    &[0; 4],
                ),
                NotOwned,
            ),
            (
                "update another mod's",
                textures.update(Some(&pool), &mut sprites, Some("beta"), &a, [2, 1], &[0; 8]),
                NotOwned,
            ),
            (
                "free another mod's",
                textures.free(Some(&pool), &mut sprites, Some("beta"), &a),
                NotOwned,
            ),
            (
                "free outside a mod",
                textures.free(Some(&pool), &mut sprites, None, &a),
                NotOwned,
            ),
            (
                "a side of 0",
                textures.create(
                    Some(&pool),
                    &mut sprites,
                    Some("alpha"),
                    &id("alpha:b"),
                    [0, 1],
                    &[],
                ),
                BadSize,
            ),
            (
                "too few bytes",
                textures.create(
                    Some(&pool),
                    &mut sprites,
                    Some("alpha"),
                    &id("alpha:b"),
                    [1, 1],
                    &[0; 3],
                ),
                BadSize,
            ),
            (
                "too many bytes on update",
                textures.update(
                    Some(&pool),
                    &mut sprites,
                    Some("alpha"),
                    &a,
                    [2, 1],
                    &[0; 9],
                ),
                BadSize,
            ),
            (
                "past the largest class",
                textures.create(
                    Some(&pool),
                    &mut sprites,
                    Some("alpha"),
                    &id("alpha:b"),
                    [1025, 1],
                    &[0; 4100],
                ),
                TooLarge,
            ),
            (
                "past the largest class on update",
                textures.update(
                    Some(&pool),
                    &mut sprites,
                    Some("alpha"),
                    &a,
                    [1, 1025],
                    &[0; 4100],
                ),
                TooLarge,
            ),
            (
                "an id its own image has",
                textures.create(
                    Some(&pool),
                    &mut sprites,
                    Some("alpha"),
                    &a,
                    [1, 1],
                    &[0; 4],
                ),
                NameTaken,
            ),
            (
                "an id a GUI sprite has",
                textures.create(
                    Some(&pool),
                    &mut sprites,
                    Some("alpha"),
                    &id("alpha:gui/panel"),
                    [1, 1],
                    &[0; 4],
                ),
                NameTaken,
            ),
            (
                "update of no image",
                textures.update(
                    Some(&pool),
                    &mut sprites,
                    Some("alpha"),
                    &id("alpha:b"),
                    [1, 1],
                    &[0; 4],
                ),
                Unknown,
            ),
            (
                "free of no image",
                textures.free(Some(&pool), &mut sprites, Some("alpha"), &id("alpha:b")),
                Unknown,
            ),
            (
                "update to a larger class",
                textures.update(
                    Some(&pool),
                    &mut sprites,
                    Some("alpha"),
                    &a,
                    [65, 1],
                    &[0; 260],
                ),
                ClassChange,
            ),
            (
                "the budget, no pool",
                textures.budget(none, Some("alpha")).map(drop),
                Unavailable,
            ),
            (
                "the budget outside a mod",
                textures.budget(Some(&pool), None).map(drop),
                NotOwned,
            ),
            (
                "over the bytes",
                textures.create(
                    Some(&pool),
                    &mut sprites,
                    Some("alpha"),
                    &id("alpha:b"),
                    [64, 65],
                    &[0; 64 * 65 * 4],
                ),
                OverBudget,
            ),
        ];
        for (case, answer, refusal) in refusals {
            assert_eq!(answer, Err(refusal), "{case}");
        }
        assert_eq!(state(&textures, &sprites), before);
        // The large slot taken by another mod: no slot of that class, and nothing changed.
        let big = vec![0; 300 * 4];
        let taken = textures.create(
            Some(&pool),
            &mut sprites,
            Some("beta"),
            &id("beta:map"),
            [300, 1],
            &big,
        );
        assert_eq!(taken, Ok(()));
        let before = state(&textures, &sprites);
        let full = textures.create(
            Some(&pool),
            &mut sprites,
            Some("alpha"),
            &id("alpha:map"),
            [300, 1],
            &big,
        );
        assert_eq!(full, Err(NoSlot));
        assert_eq!(state(&textures, &sprites), before);
    }

    /// A resource reload's new atlas: every image keeps its name, class and slot at that atlas's
    /// place for the slot, and is patched again with the pixels last given; a freed image is not.
    #[test]
    fn a_rebuilt_atlas_gets_every_kept_image_again() {
        let old = pool();
        let (mut textures, mut sprites) = (textures(), HashMap::new());
        for (name, size) in [
            ("alpha:a", [2, 1]),
            ("alpha:gone", [1, 1]),
            ("beta:b", [65, 1]),
        ] {
            let rgba = vec![3; (size[0] * size[1] * 4) as usize];
            let owner = name.split(':').next();
            let created = textures.create(Some(&old), &mut sprites, owner, &id(name), size, &rgba);
            assert_eq!(created, Ok(()));
        }
        let gone = id("alpha:gone");
        assert_eq!(
            textures.free(Some(&old), &mut sprites, Some("alpha"), &gone),
            Ok(())
        );
        textures.patches(Some(&old), &mut Vec::new());
        let new = Pool {
            origins: [
                vec![[512, 512], [576, 512]],
                vec![[0, 1024]],
                vec![[1024, 1024]],
            ],
            atlas: [2048, 2048],
        };
        // The new atlas's sprites, packed from the packs alone.
        let mut rebuilt = HashMap::new();
        textures.rebind(Some(&new), &mut rebuilt);
        let mut names: Vec<_> = rebuilt.keys().cloned().collect();
        names.sort();
        assert_eq!(names, ["alpha:a", "beta:b"]);
        assert_eq!(texels(&rebuilt, "alpha:a"), [512, 512, 514, 513, 2, 1]);
        assert_eq!(texels(&rebuilt, "beta:b"), [0, 1024, 65, 1025, 65, 1]);
        let mut patches = Vec::new();
        textures.patches(Some(&new), &mut patches);
        let placed: Vec<_> = patches
            .iter()
            .map(|p| (p.origin, p.size, p.rgba.to_vec()))
            .collect();
        assert_eq!(
            placed,
            [
                ([512, 512], [2, 1], vec![3; 8]),
                ([0, 1024], [65, 1], vec![3; 260])
            ]
        );
        // A new renderer: the same atlas, its texture made again from the packed pixels.
        textures.forget_uploads();
        let mut patches = Vec::new();
        textures.patches(Some(&new), &mut patches);
        assert_eq!(patches.len(), 2);
    }

    /// Extreme and oversized sides, with an empty pixel slice, are refused by name on create and
    /// on update, and change nothing.
    #[test]
    fn extreme_sizes_are_refused_without_overflow_and_change_nothing() {
        let pool = pool();
        let (mut textures, mut sprites) = (textures(), HashMap::new());
        let a = id("alpha:a");
        let created = textures.create(
            Some(&pool),
            &mut sprites,
            Some("alpha"),
            &a,
            [2, 1],
            &[5; 8],
        );
        assert_eq!(created, Ok(()));
        let before = state(&textures, &sprites);
        for (size, refusal) in [
            ([u32::MAX, u32::MAX], TooLarge),
            ([u32::MAX, 1], TooLarge),
            ([1, u32::MAX], TooLarge),
            ([1025, 1025], TooLarge),
            ([0, u32::MAX], BadSize),
            ([u32::MAX, 0], BadSize),
        ] {
            let b = id("alpha:b");
            let create = textures.create(Some(&pool), &mut sprites, Some("alpha"), &b, size, &[]);
            assert_eq!(create, Err(refusal), "create {size:?}");
            let update = textures.update(Some(&pool), &mut sprites, Some("alpha"), &a, size, &[]);
            assert_eq!(update, Err(refusal), "update {size:?}");
            assert_eq!(state(&textures, &sprites), before, "{size:?}");
        }
    }

    /// A frame still holding an image's first patch while the mod updates it again and again: the
    /// held patch keeps its pixels, the next frame gets exactly one patch, of the latest pixels, at
    /// the same slot, and freeing the image gives the whole budget back.
    #[test]
    fn updates_while_a_frame_holds_a_patch_leave_it_and_send_one_latest_patch() {
        let pool = pool();
        let (mut textures, mut sprites) = (textures(), HashMap::new());
        let a = id("alpha:a");
        let full = textures.budget(Some(&pool), Some("alpha"));
        let created = textures.create(
            Some(&pool),
            &mut sprites,
            Some("alpha"),
            &a,
            [2, 1],
            &[1; 8],
        );
        assert_eq!(created, Ok(()));
        // The frame in flight keeps its patch.
        let mut held = Vec::new();
        textures.patches(Some(&pool), &mut held);
        assert_eq!(held.len(), 1);
        let first_origin = held[0].origin;
        for value in 2..=9 {
            let updated = textures.update(
                Some(&pool),
                &mut sprites,
                Some("alpha"),
                &a,
                [2, 1],
                &[value; 8],
            );
            assert_eq!(updated, Ok(()));
        }
        assert_eq!(&*held[0].rgba, &[1; 8], "the held patch keeps its pixels");
        let mut next = Vec::new();
        textures.patches(Some(&pool), &mut next);
        let placed: Vec<_> = next
            .iter()
            .map(|p| (p.origin, p.size, p.rgba.to_vec()))
            .collect();
        assert_eq!(placed, [(first_origin, [2, 1], vec![9; 8])]);
        let mut after = Vec::new();
        textures.patches(Some(&pool), &mut after);
        assert!(
            after.is_empty(),
            "each image is patched once until it changes"
        );
        drop(held);
        assert_eq!(
            textures.free(Some(&pool), &mut sprites, Some("alpha"), &a),
            Ok(())
        );
        assert_eq!(textures.budget(Some(&pool), Some("alpha")), full);
        assert!(sprites.is_empty());
    }
}
