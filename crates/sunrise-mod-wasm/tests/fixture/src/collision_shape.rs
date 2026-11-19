//! The same collision part runs natively and in the compiled guest.
use mod_api::{BlockPart, CollisionShapeEvent, LoopContext};

/// Entity 7 gets a half-height slab; every other entity keeps the block's shape.
pub struct SlabForSeven;
#[mod_api::sunrise_part]
impl BlockPart for SlabForSeven {
    fn collision_shape(&self, _: &dyn LoopContext, event: &mut CollisionShapeEvent<'_>) {
        if event.query.entity() == Some(7) && !event.query.is_descending() {
            *event.shape = Some(vec![[0.0, 0.0, 0.0, 1.0, 0.5, 1.0]]);
        }
    }
}
#[cfg(target_arch = "wasm32")]
pub fn register(r: &mut dyn mod_api::Registrar, cx: &mod_api::RegistrationContext) {
    if cx.mods().get("collision_shape_probe").is_none() {
        return;
    }
    let stone = r.block_key("minecraft:stone");
    r.block_part(stone, SlabForSeven);
}
