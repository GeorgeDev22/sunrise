//! [`paint_checked`] and [`paint_items`]: an `Item` is drawn, scaled about its pivot, then its
//! decorations unscaled, under the current pose; a `Blit` of every blend is drawn with that
//! blend; an op that cannot be drawn refuses the whole buffer; and [`super::super::paint`]
//! still leaves items and screen-effect blits to its caller.
use super::super::{Blend, GuiDraw, PoseOp, paint};
use super::{GuiItems, Undrawable, paint_checked, paint_items};
use crate::hud::{Blend as Run, HudAtlas, HudBatch};

/// A run-time atlas slot every atlas has: map slot 0 (`hud::map_slot_sprite`).
const SLOT: &str = "sunrise/map0";

fn atlas() -> HudAtlas {
    HudAtlas::build(&crate::resource_source::ResourceSource::directory(
        "/nonexistent/sunrise-test-assets".into(),
    ))
    .unwrap()
}

/// Draws each slot's item as a 16 x 16 square in colour `ITEM + slot` and its decorations as a
/// 4 x 4 square in `DECORATION + slot` at the bottom right, recording each call.
#[derive(Default)]
struct Items {
    calls: Vec<(&'static str, u32, i32, i32, i32)>,
}
const ITEM: u32 = 0xFF00_0000;
const DECORATION: u32 = 0xFF10_0000;
impl GuiItems for Items {
    fn item(&mut self, batch: &mut HudBatch, slot: u32, x: i32, y: i32, seed: i32) {
        self.calls.push(("item", slot, x, y, seed));
        let atlas = atlas();
        batch.fill(&atlas, x as f32, y as f32, 16.0, 16.0, ITEM + slot);
    }
    fn decorations(&mut self, batch: &mut HudBatch, slot: u32, x: i32, y: i32) {
        self.calls.push(("decorations", slot, x, y, 0));
        let atlas = atlas();
        batch.fill(
            &atlas,
            (x + 12) as f32,
            (y + 12) as f32,
            4.0,
            4.0,
            DECORATION + slot,
        );
    }
}

fn item(slot: u32, x: i32, y: i32, scale: [f32; 2], pivot: [f32; 2]) -> GuiDraw {
    GuiDraw::Item {
        slot,
        x,
        y,
        seed: 7,
        scale,
        pivot,
    }
}

fn sprite(name: &'static str, region: Option<[i32; 4]>, blend: Blend) -> GuiDraw {
    GuiDraw::Sprite {
        sprite: name.into(),
        x: 0,
        y: 0,
        w: 4,
        h: 4,
        region,
        color: 0xFFFF_FFFF,
        blend,
    }
}

fn blit(texture: &'static str, blend: Blend, tex: [i32; 2]) -> GuiDraw {
    GuiDraw::Blit {
        texture: texture.into(),
        blend,
        x: 0,
        y: 0,
        w: 8,
        h: 8,
        uv: [0.0, 0.0],
        src: [8, 8],
        tex,
        color: 0xFFFF_FFFF,
    }
}

/// The smallest and largest corner of the vertices in `color`.
fn bounds(batch: &HudBatch, color: u32) -> [f32; 4] {
    let mut b = [
        f32::INFINITY,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NEG_INFINITY,
    ];
    for v in batch.vertices.iter().filter(|v| v.color == color) {
        b = [
            b[0].min(v.pos[0]),
            b[1].min(v.pos[1]),
            b[2].max(v.pos[0]),
            b[3].max(v.pos[1]),
        ];
    }
    b
}

#[test]
fn an_item_is_scaled_about_its_pivot_and_its_decorations_are_not() {
    let atlas = atlas();
    let mut batch = HudBatch::scratch([854, 480]);
    let mut items = Items::default();
    let draws = [item(0, 20, 20, [2.0, 2.0], [28.0, 28.0])];
    assert_eq!(
        paint_checked(&mut batch, &atlas, &draws, &mut items),
        Ok(())
    );
    assert_eq!(
        items.calls,
        [("item", 0, 20, 20, 7), ("decorations", 0, 20, 20, 0)]
    );
    assert_eq!(bounds(&batch, ITEM), [12.0, 12.0, 44.0, 44.0]);
    assert_eq!(bounds(&batch, DECORATION), [32.0, 32.0, 36.0, 36.0]);
}

#[test]
fn items_follow_the_current_pose_and_every_hotbar_slot_and_the_off_hand_draw() {
    let atlas = atlas();
    let mut batch = HudBatch::scratch([854, 480]);
    let mut items = Items::default();
    let draws = [
        GuiDraw::Push(vec![PoseOp::Translate(100.0, 10.0)]),
        item(8, 0, 0, [1.0, 1.0], [8.0, 12.0]),
        GuiDraw::Pop,
        item(40, 0, 0, [1.0, 1.0], [8.0, 12.0]),
    ];
    assert_eq!(
        paint_checked(&mut batch, &atlas, &draws, &mut items),
        Ok(())
    );
    assert_eq!(bounds(&batch, ITEM + 8), [100.0, 10.0, 116.0, 26.0]);
    assert_eq!(bounds(&batch, DECORATION + 8), [112.0, 22.0, 116.0, 26.0]);
    assert_eq!(bounds(&batch, ITEM + 40), [0.0, 0.0, 16.0, 16.0]);
}

#[test]
fn every_blit_blend_draws_with_its_blend() {
    let atlas = atlas();
    let mut batch = HudBatch::scratch([854, 480]);
    let draws = [
        blit(SLOT, Blend::Textured, [8, 8]),
        blit(SLOT, Blend::Vignette, [8, 8]),
        blit(SLOT, Blend::Nausea, [8, 8]),
        blit(SLOT, Blend::Crosshair, [8, 8]),
    ];
    assert_eq!(
        paint_checked(&mut batch, &atlas, &draws, &mut Items::default()),
        Ok(())
    );
    assert_eq!(batch.vertices.len(), 24);
    assert_eq!(
        batch.runs(),
        [
            (0..6, Run::Alpha),
            (6..12, Run::Vignette),
            (12..18, Run::Additive),
            (18..24, Run::Invert),
        ]
    );
}

#[test]
fn an_op_the_paint_cannot_draw_refuses_the_buffer_and_draws_nothing_of_it() {
    let atlas = atlas();
    let sprite = GuiDraw::Sprite {
        sprite: "test:missing".into(),
        x: 0,
        y: 0,
        w: 4,
        h: 4,
        region: None,
        color: 0xFFFF_FFFF,
        blend: Blend::Textured,
    };
    let atlas_sprite = GuiDraw::AtlasSprite {
        sprite: "test:missing".into(),
        x: 0,
        y: 0,
        w: 4,
        h: 4,
        color: 0xFFFF_FFFF,
    };
    let cases = [
        (
            item(9, 0, 0, [1.0, 1.0], [0.0, 0.0]),
            Undrawable::ItemSlot(9),
        ),
        (
            item(41, 0, 0, [1.0, 1.0], [0.0, 0.0]),
            Undrawable::ItemSlot(41),
        ),
        (
            item(0, 0, 0, [f32::NAN, 1.0], [0.0, 0.0]),
            Undrawable::ItemNotFinite,
        ),
        (
            item(0, 0, 0, [1.0, 1.0], [0.0, f32::INFINITY]),
            Undrawable::ItemNotFinite,
        ),
        (
            blit("test:missing", Blend::Vignette, [8, 8]),
            Undrawable::BlitTexture("test:missing".into()),
        ),
        (blit(SLOT, Blend::Textured, [0, 8]), Undrawable::BlitSize),
        (sprite, Undrawable::Sprite("test:missing".into())),
        (atlas_sprite, Undrawable::Sprite("test:missing".into())),
    ];
    assert_eq!(refused_alone(&atlas, cases.to_vec()), cases.len());
}

/// Each case refused after a fill and an item, at index 2, with nothing of the buffer drawn;
/// answers how many were checked.
fn refused_alone(atlas: &HudAtlas, cases: Vec<(GuiDraw, Undrawable)>) -> usize {
    let fill = GuiDraw::Fill {
        x0: 0,
        y0: 0,
        x1: 4,
        y1: 4,
        color: 0xFFFF_FFFF,
    };
    let checked = cases.len();
    for (op, why) in cases {
        let mut batch = HudBatch::scratch([854, 480]);
        let mut items = Items::default();
        let draws = [fill.clone(), item(0, 0, 0, [1.0, 1.0], [0.0, 0.0]), op];
        assert_eq!(
            paint_checked(&mut batch, atlas, &draws, &mut items),
            Err((2, why))
        );
        assert!(batch.vertices.is_empty());
        assert!(items.calls.is_empty());
    }
    checked
}

#[test]
fn a_sprite_region_of_zero_width_or_height_is_refused_before_anything_divides_by_it() {
    let atlas = atlas();
    let cases = vec![
        (
            sprite("hud/heart/full", Some([0, 9, 0, 0]), Blend::Textured),
            Undrawable::SpriteRegion,
        ),
        (
            sprite("hud/heart/full", Some([9, 0, 0, 0]), Blend::Textured),
            Undrawable::SpriteRegion,
        ),
        (
            sprite("hud/heart/full", Some([-1, 9, 0, 0]), Blend::Crosshair),
            Undrawable::SpriteRegion,
        ),
    ];
    assert_eq!(refused_alone(&atlas, cases), 3);
}

#[test]
fn a_sprite_with_a_screen_effect_blend_is_refused_not_drawn_plain() {
    let atlas = atlas();
    let cases = vec![
        (
            sprite("hud/heart/full", None, Blend::Nausea),
            Undrawable::SpriteBlend(Blend::Nausea),
        ),
        (
            sprite("hud/heart/full", Some([9, 9, 0, 0]), Blend::Vignette),
            Undrawable::SpriteBlend(Blend::Vignette),
        ),
    ];
    assert_eq!(refused_alone(&atlas, cases), 2);
}

#[test]
fn a_missing_mob_effect_sprite_plain_or_namespaced_is_refused_as_unknown() {
    let atlas = atlas();
    let cases = vec![
        (
            sprite("mob_effect/test_missing", None, Blend::Textured),
            Undrawable::Sprite("mob_effect/test_missing".into()),
        ),
        (
            sprite("test:mob_effect/missing", None, Blend::Textured),
            Undrawable::Sprite("test:mob_effect/missing".into()),
        ),
    ];
    assert_eq!(refused_alone(&atlas, cases), 2);
}

#[test]
fn plain_paint_still_leaves_items_and_screen_effect_blits_to_its_caller() {
    let atlas = atlas();
    let mut batch = HudBatch::scratch([854, 480]);
    let draws = [
        item(0, 0, 0, [1.0, 1.0], [0.0, 0.0]),
        blit(SLOT, Blend::Vignette, [8, 8]),
        blit(SLOT, Blend::Nausea, [8, 8]),
        blit(SLOT, Blend::Crosshair, [8, 8]),
    ];
    paint(&mut batch, &atlas, &draws);
    assert!(batch.vertices.is_empty());
    // The hotbar's path draws them.
    let mut items = Items::default();
    paint_items(&mut batch, &atlas, &draws, &mut items);
    assert_eq!(items.calls.len(), 2);
    assert_eq!(batch.vertices.len(), 6 * 3 + 6 * 2);
}
