//! The `enchant_effect` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0090

pub trait Hooks {
    /// A `minecraft:post_attack` effect of a mod's own kind, after a melee hit
    /// landed: a player's hit on a mob or another player, or an armed mob's hit
    /// on a mob or a player. The enchantment's `slots` include the main hand,
    /// the entry says `enchanted` attacker and `affected` victim, its
    /// `requirements` passed (judged with the hit's damage type), and its
    /// `effect` names a `type` in a namespace outside `minecraft:`. Told the enchantment id, the kind, the
    /// `effect` payload as a borrowed `Tag`, the level, the `attacker`, the
    /// `victim` (it may have just died) and the enchanted `weapon`: the stack
    /// the attacker hit with (`getWeaponItem`, taken before the hit; a spear
    /// stab's own stack), which may since have broken or been dropped. Each such entry is told once a
    /// hit, and once for each mob or player a sweep lands on. An entry inside
    /// `minecraft:all_of` isn't told yet. An unclaimed kind does nothing.
    #[hook(
        target = "sunrise_client::game::native_hooks::base::Game<'_>::native_enchant_effect",
        side = "server",
        contract = "listen",
        version = 2,
        neoforge = "RegisterEvent"
    )]
    fn enchant_effect(
        enchantment: &str,
        kind: &str,
        data: &sunrise_core::nbt::Tag,
        level: u32,
        attacker: mod_api::EntityRef,
        victim: mod_api::EntityRef,
        weapon: &ItemStack,
    );
}
