//! A mod's general entity rows on the window (#792, stage 5): each row the server shows, with its
//! first synced fields here (`game/network/remote/general.rs`), drawn by the renderer a mod
//! registered for its type (`register_entity_renderers`), with a frozen copy of the row's synced
//! attachments. A general type has no look of its own: without a renderer, or
//! with one that failed until the next resource load, its rows draw nothing.
use super::{Game, entity_culling};
use glam::DVec3;
use sunrise_client::mesher::Vertex;
use sunrise_core::geometry::Vec3;

/// `EntityRenderState` for a general row: its identity, age and pose at the frame's `partial`
/// tick, interpolated from the tick before as `Entity.getYRot(partial)` does, with no family's
/// fields.
fn render_state(
    id: u32,
    row: &super::remote::general::RemoteGeneral,
    partial: f32,
) -> mod_api::EntityRenderState {
    let lerp = |from: f32, to: f32| from + partial * (to - from);
    mod_api::__private::built::EntityRenderState {
        id,
        partial,
        age: row.age as f32 + partial,
        yaw: lerp(row.previous_rotation[0], row.rotation[0]),
        pitch: lerp(row.previous_rotation[1], row.rotation[1]),
        living: None,
        tnt_fuse: None,
        orb_value: None,
        painting: None,
        item_frame: None,
        attachments: row.render_attachments.clone(),
    }
    .into()
}

/// A visible row prepared for submission. Nothing here reads live mod state.
pub(super) struct ExtractedGeneral {
    type_slot: usize,
    entity: mod_api::EntityRenderState,
    at: DVec3,
    block: [i32; 3],
    light: u32,
}

impl Game<'_> {
    /// Draws the general rows shown on the window at the frame's `alpha`, `camera` the eye: each
    /// from `lerp(previous, position, alpha)`, lit as the entity pass lights an entity, by its
    /// eye (`EntityRenderer.getPackedLightCoords`, at vanilla's default eye height of 0.85 of
    /// the declared height). With no renderer registered for any general type, one flag check.
    pub(super) fn general_entity_geometry(
        &self,
        camera: glam::DVec3,
        alpha: f64,
        out: &mut Vec<Vertex>,
    ) {
        let Some(renderers) = super::native_entity_renderers::general_renderers() else {
            return;
        };
        let Some(remote) = self.remote.as_ref() else {
            return;
        };
        let catalog = super::general::general_type_catalog();
        let partial = alpha as f32;
        let view_scale = self.view_scale();
        let now = std::time::Instant::now();
        // Extract the visible list before invoking mod code, as LevelExtractor does. The
        // client's lookup preserves insertion order (vanilla EntityLookup's linked id map).
        let mut visible = std::mem::take(&mut *self.general_render_states.borrow_mut());
        for (&id, row) in remote.general.rows.iter().filter(|(_, row)| row.shown) {
            if !renderers
                .general(row.type_slot)
                .is_some_and(|renderer| !renderer.failed.load(std::sync::atomic::Ordering::Relaxed))
            {
                continue;
            }
            let position = DVec3::from_array(row.position);
            let previous = DVec3::from_array(row.previous);
            let at = previous + (position - previous) * alpha;
            let bounds =
                catalog.bounds(row.type_slot, Vec3::new(position.x, position.y, position.z));
            let range =
                entity_culling::default_range(entity_culling::at(bounds.size()), view_scale);
            let offset = at - position;
            if !self.entity_in_view(
                camera,
                position,
                range,
                entity_culling::at(bounds.min_position()) + offset,
                entity_culling::at(bounds.max_position()) + offset,
            ) {
                continue;
            }
            let section_block = position.floor().as_ivec3().to_array();
            let y = section_block[1];
            if self.world.shape.contains_y(y)
                && !self.renderer.as_ref().is_some_and(|renderer| {
                    renderer.entity_section_visible(
                        section_block,
                        self.settings.chunk_fade as f32,
                        now,
                    )
                })
            {
                continue;
            }
            let entity = render_state(id as u32, row, partial);
            let block = at.floor().as_ivec3().to_array();
            let (_, height) = catalog.dimensions(row.type_slot);
            let eye = (at.y + height * 0.85).floor() as i32;
            let light = self.world.light(block[0], eye, block[2]);
            let packed = (u32::from(light >> 4) * 16) | ((u32::from(light & 15) * 16) << 8);
            visible.push(ExtractedGeneral {
                type_slot: row.type_slot,
                entity,
                at,
                block,
                light: packed,
            });
        }
        for row in visible.drain(..) {
            if let Some(resolved) = renderers.general(row.type_slot) {
                self.mod_entity_renderer_geometry(
                    resolved,
                    &row.entity,
                    catalog.key(row.type_slot),
                    row.at - camera,
                    Some(row.at),
                    row.block,
                    row.light,
                    out,
                );
            }
        }
        *self.general_render_states.borrow_mut() = visible;
    }
}

#[cfg(test)]
mod tests {
    use super::render_state;

    /// The frame between two ticks: a quarter of the way, in age, yaw and pitch alike, with no
    /// family's fields.
    #[test]
    fn a_rows_state_is_interpolated_to_the_frame() {
        let row = super::super::remote::general::RemoteGeneral {
            type_slot: 0,
            uuid: 1,
            position: [0.0; 3],
            previous: [0.0; 3],
            rotation: [90.0, 20.0],
            previous_rotation: [0.0, -20.0],
            age: 40,
            shown: true,
            render_attachments: None,
        };
        let state = render_state(9, &row, 0.25);
        assert_eq!(state.id, 9);
        assert_eq!(state.age, 40.25);
        assert_eq!((state.yaw, state.pitch), (22.5, -10.0));
        assert!(state.living.is_none() && state.tnt_fuse.is_none() && state.painting.is_none());
    }
}
