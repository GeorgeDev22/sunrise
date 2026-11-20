//! The settings screens' look: vanilla's widget sprites and colours, and the shell's sizes.
use crate::menu_style::{self, NEUTRAL, SettingsColours};
use crate::{
    hud::{HudAtlas, HudBatch},
    text::Text,
    ui,
};

/// Vanilla's greys and chat colours (`menu_style::NEUTRAL`).
fn b() -> &'static SettingsColours {
    &NEUTRAL.settings
}

/// What a setting costs in frame time, shown with its description.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cost {
    None,
    Low,
    Medium,
    High,
    /// Turning it on makes frames faster (culling, caching, batching).
    Saves,
}
impl Cost {
    pub fn label(self) -> &'static str {
        match self {
            Cost::None => "No cost",
            Cost::Low => "Low cost",
            Cost::Medium => "Medium cost",
            Cost::High => "High cost",
            Cost::Saves => "Saves time",
        }
    }
    /// `0xRRGGBB`: vanilla's green, yellow and red.
    pub fn rgb(self) -> u32 {
        match self {
            Cost::None => b().ink_soft,
            Cost::Low | Cost::Saves => b().success,
            Cost::Medium => b().warn,
            Cost::High => b().danger,
        }
    }
}

use menu_style::hud_colour as hex;
/// A hovered or focused row.
pub fn hover() -> u32 {
    hex(b().ink, 18)
}
/// The open page in the page list.
pub fn open() -> u32 {
    hex(b().ink, 26)
}
/// The rule under the plate's heading, and between two rows of a group.
pub fn head_rule() -> u32 {
    hex(b().ink, 31)
}
pub fn row_rule() -> u32 {
    hex(b().ink, 15)
}
pub fn accent() -> u32 {
    hex(b().accent, 255)
}
/// Vanilla's white text, and its grey for what can't be used now.
pub const WHITE: u32 = u32::MAX;
pub fn muted() -> u32 {
    hex(b().muted, 255)
}
/// A page that isn't open, a value at rest.
pub fn soft() -> u32 {
    hex(b().ink_soft, 255)
}
/// Vanilla's grey for a tooltip's lines under its first (`ChatFormatting.GRAY`).
pub const GREY: u32 = hex(0xaaaaaa, 255);

/// Sizes in GUI pixels.
/// Shorter screens than this (a 720p window at scale 3) keep a narrower margin on top.
pub const TITLED: f32 = 300.0;
pub const ROW: f32 = 20.0;
/// A group's name over its panel, and a page's over its groups while searching.
pub const GROUP_ROW: f32 = 14.0;
pub const PAGE_ROW: f32 = 16.0;
/// Space under a group's panel, and more before the next page's name.
pub const GROUP_GAP: f32 = 10.0;
pub const PAGE_GAP: f32 = 6.0;
/// The control on the right of a row (a button, a box or a slider), and the
/// left column: the search field, the page list and the buttons.
pub const CONTROL_W: f32 = 112.0;
pub const CONTROL_H: f32 = 16.0;
pub const SEARCH_H: f32 = 16.0;
/// Vanilla's scroller: the column `ui::Scroll::press` takes.
pub const SCROLLER_W: f32 = 6.0;
/// The strip under the plate that describes a setting.
pub const DOCK_H: f32 = 50.0;
pub const BUTTON_H: f32 = 20.0;
/// A row's name and control, from its group panel's edges.
pub const INSET: f32 = 10.0;

pub fn fill(batch: &mut HudBatch, atlas: &HudAtlas, [x, y, w, h]: [f32; 4], color: u32) {
    batch.fill(atlas, x, y, w, h, color);
}
/// A 1 px ring inside `rect`: the keyboard or controller focus, in white as vanilla draws it.
pub fn ring(batch: &mut HudBatch, atlas: &HudAtlas, [x, y, w, h]: [f32; 4], color: u32) {
    batch.fill(atlas, x, y, w, 1.0, color);
    batch.fill(atlas, x, y + h - 1.0, w, 1.0, color);
    batch.fill(atlas, x, y + 1.0, 1.0, h - 2.0, color);
    batch.fill(atlas, x + w - 1.0, y + 1.0, 1.0, h - 2.0, color);
}
/// The plate and the strip under it: vanilla's list background, as options lists draw it.
pub fn panel(batch: &mut HudBatch, atlas: &HudAtlas, [x, y, w, h]: [f32; 4], in_world: bool) {
    let sprite = if in_world {
        "gui/inworld_menu_list_background"
    } else {
        "gui/menu_list_background"
    };
    ui::tiled(batch, atlas, sprite, x, y, w, h, 32.0, WHITE);
}
/// The search field and other wells: vanilla's text field, highlighted while focused.
pub fn well(batch: &mut HudBatch, atlas: &HudAtlas, [x, y, w, h]: [f32; 4], focused: bool) {
    let sprite = if focused {
        "gui/sprites/widget/text_field_highlighted"
    } else {
        "gui/sprites/widget/text_field"
    };
    ui::nine_slice(batch, atlas, sprite, x, y, w, h, WHITE);
}
/// Vanilla's scroller: its background on the track, the scroller on the thumb.
pub fn scroller(batch: &mut HudBatch, atlas: &HudAtlas, track: [f32; 4], thumb: [f32; 4]) {
    let [x, y, w, h] = track;
    ui::nine_slice(
        batch,
        atlas,
        "gui/sprites/widget/scroller_background",
        x,
        y,
        w,
        h,
        WHITE,
    );
    let [x, y, w, h] = thumb;
    ui::nine_slice(
        batch,
        atlas,
        "gui/sprites/widget/scroller",
        x,
        y,
        w,
        h,
        WHITE,
    );
}
/// `text` cut to `width` with an ellipsis, as a long name gives way to its control: the HUD
/// kit's `fit::ellipsize`.
pub use crate::hud_kit::fit::ellipsize as fit;
/// Vanilla's 17 px check box ending at `right`, centred in the row at `y`; `lit` when pointed at or focused.
pub fn checkbox(
    batch: &mut HudBatch,
    atlas: &HudAtlas,
    right: f32,
    y: f32,
    on: bool,
    (lit, live): (bool, bool),
) {
    let sprite = match (on, lit && live) {
        (true, true) => "gui/sprites/widget/checkbox_selected_highlighted",
        (true, false) => "gui/sprites/widget/checkbox_selected",
        (false, true) => "gui/sprites/widget/checkbox_highlighted",
        (false, false) => "gui/sprites/widget/checkbox",
    };
    let tint = if live { WHITE } else { muted() };
    let top = y + ((ROW - 17.0) / 2.0).floor();
    ui::nine_slice(batch, atlas, sprite, right - 17.0, top, 17.0, 17.0, tint);
}
/// Vanilla's button with its label centred; pointed at or focused, its highlighted sprite.
pub fn button(
    batch: &mut HudBatch,
    atlas: &HudAtlas,
    rect: [f32; 4],
    label: &Text,
    flags: (bool, bool, bool),
) {
    button_with_room(batch, atlas, rect, label, flags, 0.0);
}
/// A button whose label leaves `room` at its left, for a controller's glyph drawn there
/// (`widgets::action_glyph_room`).
pub fn button_with_room(
    batch: &mut HudBatch,
    atlas: &HudAtlas,
    rect: [f32; 4],
    label: &Text,
    (hover, live, _): (bool, bool, bool),
    room: f32,
) {
    let [x, y, w, h] = rect;
    let sprite = match (live, hover) {
        (false, _) => "gui/sprites/widget/button_disabled",
        (true, true) => "gui/sprites/widget/button_highlighted",
        (true, false) => "gui/sprites/widget/button",
    };
    ui::nine_slice(batch, atlas, sprite, x, y, w, h, WHITE);
    let color = if live { WHITE } else { muted() };
    let ty = y + ((h - 8.0) / 2.0).floor();
    let left = x + 2.0 + room;
    scrolling_label(
        batch,
        atlas,
        label,
        [left, ty, w - 4.0 - room],
        (y, h),
        color,
    );
}
/// A widget label of `[left, y, width]` inside the widget's `(top, height)`: centred when it fits,
/// else slid (vanilla `AbstractWidget.extractScrollingStringOverContents`) and clipped to the box.
fn scrolling_label(
    batch: &mut HudBatch,
    atlas: &HudAtlas,
    text: &Text,
    [left, y, width]: [f32; 3],
    (top, height): (f32, f32),
    color: u32,
) {
    ui::scrolling_text(
        batch,
        atlas,
        text,
        left,
        left + width,
        top,
        top + height,
        y,
        color,
    );
}
/// What a slider's sprites follow, as `AbstractSliderButton.getSprite` and `getHandleSprite`.
#[derive(Clone, Copy, Debug, Default)]
pub struct SliderState {
    pub hover: bool,
    pub focused: bool,
    /// `canChangeValue`: the keyboard changes its value (Enter pressed on it).
    pub editing: bool,
    /// Dragged with the mouse.
    pub held: bool,
    pub live: bool,
}
/// Vanilla's slider: track and 8 px handle at `position` (0..1) lit as `AbstractSliderButton`'s, the value over them.
pub fn slider(
    batch: &mut HudBatch,
    atlas: &HudAtlas,
    rect: [f32; 4],
    value: &Text,
    position: f32,
    state: SliderState,
) {
    let [x, y, w, h] = rect;
    let live = state.live;
    let tint = if live { WHITE } else { muted() };
    let (track, handle) =
        ui::slider_sprites_ext(state.hover, live, state.focused, state.editing, state.held);
    ui::nine_slice(batch, atlas, track, x, y, w, h, tint);
    let at = x + (position.clamp(0.0, 1.0) * (w - 8.0)).floor();
    ui::nine_slice(batch, atlas, handle, at, y, 8.0, h, tint);
    let ty = y + ((h - 8.0) / 2.0).floor();
    let color = if live { WHITE } else { muted() };
    scrolling_label(batch, atlas, value, [x + 2.0, ty, w - 4.0], (y, h), color);
}
/// A value that a press steps through: a button showing the value, as vanilla's `CycleButton`.
pub fn cycler(
    batch: &mut HudBatch,
    atlas: &HudAtlas,
    rect: [f32; 4],
    value: &Text,
    (hover, live): (bool, bool),
) {
    button_with_room(batch, atlas, rect, value, (hover, live, false), 0.0);
}
/// A screen's title, centred at the top.
pub fn title(batch: &mut HudBatch, atlas: &HudAtlas, text: &Text) {
    let w = batch.width;
    ui::centered_text(batch, atlas, text, 0.0, 12.0, w, u32::MAX);
}
/// The cost's colour in the HUD's encoding: the square beside its name.
pub fn cost_color(cost: Cost) -> u32 {
    hex(cost.rgb(), 255)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn atlas() -> HudAtlas {
        HudAtlas::build(&sunrise_assets::resource_source::ResourceSource::directory(
            (std::path::Path::new("/nonexistent/sunrise-test-assets")).to_owned(),
        ))
        .unwrap()
    }

    fn x_span(batch: &HudBatch, plain: &HudBatch) -> (f32, f32) {
        let label = |v: &&crate::hud::HudVertex| !plain.vertices.iter().any(|p| p.pos == v.pos);
        batch
            .vertices
            .iter()
            .filter(label)
            .map(|v| v.pos[0])
            .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), x| {
                (lo.min(x), hi.max(x))
            })
    }

    /// A label wider than its widget slides inside the widget's box instead of being cut by
    /// characters; a short one stays centred.
    #[test]
    fn long_labels_stay_in_the_widget_and_short_ones_centred() {
        let atlas = atlas();
        let rect = [10.0, 20.0, 100.0, 20.0];
        let long = Text::literal("Graphics API: Prefer Vulkan (Experimental)");
        assert!(atlas.font.width_text(&long.resolved()) > rect[2]);
        let short = Text::literal("Done");
        let short_w = atlas.font.width_text(&short.resolved());
        let draws: [(&str, fn(&mut HudBatch, &HudAtlas, [f32; 4], &Text)); 3] = [
            ("button", |b, a, r, t| {
                button(b, a, r, t, (false, true, false))
            }),
            ("slider", |b, a, r, t| {
                slider(
                    b,
                    a,
                    r,
                    t,
                    0.5,
                    SliderState {
                        live: true,
                        ..Default::default()
                    },
                )
            }),
            ("cycler", |b, a, r, t| cycler(b, a, r, t, (false, true))),
        ];
        for (name, draw) in draws {
            let mut plain = HudBatch::scratch([640, 480]);
            draw(&mut plain, &atlas, rect, &Text::empty());
            let base = plain.vertices.len();
            let mut batch = HudBatch::scratch([640, 480]);
            draw(&mut batch, &atlas, rect, &long);
            assert!(batch.vertices.len() > base, "{name}: long label drawn");
            // The widget's own fill, track and handle also draw: only the label's vertices count.
            let label = batch
                .vertices
                .iter()
                .filter(|v| !plain.vertices.iter().any(|p| p.pos == v.pos));
            for v in label {
                assert!(
                    (rect[0]..=rect[0] + rect[2]).contains(&v.pos[0])
                        && (rect[1]..=rect[1] + rect[3]).contains(&v.pos[1]),
                    "{name}: {:?} outside the widget",
                    v.pos
                );
            }
            // The label's own glyphs (past the widget's fill) start centred.
            let mut batch = HudBatch::scratch([640, 480]);
            draw(&mut batch, &atlas, rect, &short);
            let (lo, hi) = x_span(&batch, &plain);
            let middle = (lo + hi) / 2.0;
            let want = rect[0] + rect[2] / 2.0;
            assert!(
                (middle - want).abs() <= short_w / 2.0 + 2.0 && (middle - want).abs() < 3.0,
                "{name}: centre {middle}, want {want}"
            );
        }
    }
}
