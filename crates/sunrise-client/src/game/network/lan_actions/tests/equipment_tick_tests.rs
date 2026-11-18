//! `LivingEntity.detectEquipmentUpdates` runs in the player's tick, after the tick's packets:
//! a slot change and an attack sent in one tick act with the old item's attributes (the
//! attribute swap, #637, #808).
use super::*;
use crate::game::native_entity_refs::tests::with_game;
use crate::game::native_item_places::tests::{guest, open_to_lan};
use std::sync::Arc;
use sunrise_client::chunk::Chunk;

#[test]
#[ignore = "daily"]
fn a_new_slot_takes_its_items_attributes_at_the_guests_tick() {
    with_game(|game| {
        open_to_lan(game);
        game.world
            .chunks
            .insert((0, 0), Arc::new(Chunk::empty(0, 0)));
        let mut g = guest(1);
        g.loaded = true;
        g.position = [8.5, 64.0, 8.5];
        g.inventory[0] = Some(ItemStack::new("minecraft:diamond_sword", 1));
        g.inventory[1] = Some(ItemStack::new("minecraft:iron_spear", 1));
        g.selected = 0;
        game.lan.as_mut().unwrap().hub.guests.extend([g]);
        game.tick_guests();
        let damage = |game: &mut Game<'_>| game.as_guest(1, |g| g.attack_damage()).unwrap();
        // The player's 1 and the diamond sword's base_attack_damage of 6.
        assert_eq!(damage(game), 7.0);
        game.lan_guest_packet(1, S::SetCarriedItem(play::SetCarriedItem { slot: 1 }));
        assert_eq!(
            damage(game),
            7.0,
            "the sword's modifiers stay until the tick"
        );
        game.tick_guests();
        // The iron spear's base_attack_damage is 2.
        assert_eq!(damage(game), 3.0);
    });
}
