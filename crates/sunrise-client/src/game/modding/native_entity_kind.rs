//! Entity-type key reads share the completed registry and never format a type name.
use super::{Game, commands::Target};
use mod_api::EntityTypeKey;
impl Game<'_> {
    pub(super) fn native_entity_kind_key(&self, target: Target) -> Option<EntityTypeKey> {
        let name = match target {
            Target::Player | Target::Guest(_) => "minecraft:player",
            Target::Mob(id) => &self.mobs.iter().find(|mob| mob.id == id)?.kind.id,
            Target::Item(_) => "minecraft:item",
            Target::Orb(_) => "minecraft:experience_orb",
            Target::Projectile(id) => self
                .creatures
                .projectiles
                .iter()
                .find(|p| p.id == id)?
                .entity_type(),
            Target::Stand(_) => "minecraft:armor_stand",
            Target::General(id) => self.general_kind(id)?,
            Target::Frame(id) => {
                if self
                    .item_frames
                    .iter()
                    .any(|frame| frame.id == id && frame.glow)
                {
                    "minecraft:glow_item_frame"
                } else {
                    "minecraft:item_frame"
                }
            }
            Target::Painting(_) => "minecraft:painting",
            Target::Boat(id) => self.boats.iter().find(|b| b.id == id)?.entity_type_name(),
            Target::Cart(id) => self.carts.iter().find(|cart| cart.id == id)?.kind.id(),
            Target::Other(id) => &self.others.iter().find(|other| other.id == id)?.kind,
            Target::Crystal(_) => "minecraft:end_crystal",
            Target::Dragon(_) | Target::DragonPart(_) => "minecraft:ender_dragon",
            Target::Tnt(_) => "minecraft:tnt",
            Target::Cushion(_) => "minecraft:cushion",
            Target::Falling(_) => "minecraft:falling_block",
            Target::Rocket(_) => "minecraft:firework_rocket",
            Target::Trident(_) => "minecraft:trident",
            Target::Fangs(_) => "minecraft:evoker_fangs",
            Target::Spawner(_) => "minecraft:ominous_item_spawner",
            Target::Cloud(_) => "minecraft:area_effect_cloud",
            Target::Hook(_) => "minecraft:fishing_bobber",
            Target::Knot(_) => "minecraft:leash_knot",
        };
        mod_api::__private::resolved_entity_type_key(name)
    }
}
