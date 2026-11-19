use super::*;

impl Game<'_> {
    /// Send one selected attack with the same prediction and charge bookkeeping as vanilla.
    pub(in crate::game) fn remote_attack_entity(&mut self, id: i32, distance: f64) -> bool {
        if let Some(index) = self.mobs.iter().position(|mob| mob.id as i32 == id) {
            if !self.held_range_allows(distance) {
                return false;
            }
            self.send(S::Attack(play::Attack { id }));
            let kind = format!(
                "minecraft:{}",
                self.mobs[index].kind.id.trim_start_matches("minecraft:")
            );
            self.remote_sweep_event(&kind, true);
            self.pvp_attacked(id, distance);
            self.remote_player_attack(index);
        } else {
            self.send(S::Attack(play::Attack { id }));
            if let Some(projectile) = self
                .creatures
                .projectiles
                .iter()
                .find(|p| p.id as i32 == id)
            {
                if !crate::game::deflect::redirectable(projectile) {
                    let bullet = matches!(
                        projectile.kind,
                        sunrise_client::projectiles::ProjectileKind::ShulkerBullet { .. }
                    );
                    self.remote_sweep_event("minecraft:shulker_bullet", false);
                    if bullet {
                        self.remote_object_attack();
                    }
                }
            } else if let Some((kind, living)) = self.remote_attacked_object(id) {
                self.remote_sweep_event(&kind, living);
                // A boat, a minecart or an End crystal; an armor stand is
                // living, and its hurtClient is false.
                if !living {
                    self.remote_object_attack();
                }
            }
        }
        self.reset_attack_strength();
        true
    }
    /// `Player.attack`'s damage and `getAttackStrengthScale(0.5)` on this
    /// client.
    pub(super) fn remote_attack_damage(&mut self) -> (f32, f32) {
        // `getAttackStrengthScale(0.5)`: here the ticker is `hand_strength`'s
        // (`attack_ticks` only counts in the local world), and the attack
        // speed is the player's synced attribute, which a server may change.
        let speed = self
            .own_attribute("minecraft:attack_speed")
            .unwrap_or_else(|| self.main_hand_attack_speed());
        let strength = self.hand_strength.scale(0.5, speed);
        // Player.attack's damage: a riptide spin's, or this client's own
        // ATTACK_DAMAGE, which the held item and effects don't change here.
        let spin = self.player.as_ref().map_or(0.0, |p| p.auto_spin_attack_dmg);
        let damage = if spin > 0.0 {
            spin
        } else {
            self.own_attribute("minecraft:attack_damage")
                .map_or(1.0, |v| v as f32)
        };
        (damage, strength)
    }
    /// The attacker's side of `causeExtraKnockback` after a hit whose
    /// `hurtClient` is true on this client (another player, a boat, a
    /// minecart, an End crystal, a shulker bullet): with any knockback, the
    /// 0.6 horizontal speed cut and the sprint stop, which that tick's
    /// `sendChanges` sends. Without it the next move outruns the server's
    /// player.
    pub(super) fn remote_attacker_knockback(&mut self, damage: f32, strength: f32) {
        let knockback = self
            .own_attribute("minecraft:attack_knockback")
            .map_or(0.0, |v| v as f32);
        let sprinting = self.player.as_ref().is_some_and(|p| p.sprinting);
        if !sunrise_client::attack_strength::attacker_slows_after_hit(
            damage, knockback, strength, sprinting,
        ) {
            return;
        }
        if let Some(p) = self.player.as_mut() {
            let slowdown = sunrise_client::attack_strength::ATTACKER_SLOWDOWN;
            p.velocity = Vec3::new(
                p.velocity.x * slowdown,
                p.velocity.y,
                p.velocity.z * slowdown,
            );
        }
        self.stop_sprint_after_extra_knockback();
    }
    /// A hit on a boat, minecart, End crystal or shulker bullet: their
    /// `hurtClient` is true (`VehicleEntity`, `EndCrystal` for any source but
    /// the dragon, `ShulkerBullet`), so `Player.attack` reaches
    /// `causeExtraKnockback` here as for a player.
    fn remote_object_attack(&mut self) {
        let (damage, strength) = self.remote_attack_damage();
        self.remote_attacker_knockback(damage, strength);
    }
}

#[cfg(test)]
mod tests {
    use crate::game::boats::Boat;
    use crate::game::native_entity_refs::tests::with_client_game;
    use crate::game::native_hotbar_slot::hotbar_slot_tests::{logged_in, sent};
    use std::sync::Arc;
    use sunrise_core::geometry::Vec3;
    use sunrise_protocol::packets::play::Serverbound as S;

    #[test]
    #[ignore = "daily"]
    fn a_sprint_hit_on_a_boat_slows_the_attacker_and_stops_its_sprint() {
        with_client_game(|game| {
            let mut reader = logged_in(game);
            game.boats.push(Boat::new(
                101,
                Arc::from("oak"),
                Vec3::new(0.5, 65.0, 2.5),
                0.0,
            ));
            // Full strength (getAttackStrengthScale above 0.9) while sprinting:
            // Player.attack's knockback is 0.5, and VehicleEntity.hurtClient is true.
            game.hand_strength.ticker = 100;
            let p = game.player.as_mut().unwrap();
            let _ = p.set_sprinting(true);
            p.velocity = Vec3::new(0.0, 0.0, 0.25);
            assert!(game.remote_attack_entity(101, 2.0));
            let p = game.player.as_ref().unwrap();
            assert!(!p.sprinting, "causeExtraKnockback stops the sprint");
            assert_eq!(p.velocity, Vec3::new(0.0, 0.0, 0.25 * 0.6));
            assert!(
                sent(&mut reader)
                    .iter()
                    .any(|packet| matches!(packet, S::Attack(a) if a.id == 101)),
                "the attack goes out"
            );
        });
    }
}
