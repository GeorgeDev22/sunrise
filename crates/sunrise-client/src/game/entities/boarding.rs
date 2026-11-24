//! A player's `Entity.boardingCooldown` is always 0: `Player.removeVehicle`
//! zeroes the 60 ticks `Entity.removePassenger` just set (#11158), so
//! `Entity.canRide` only asks that the player is not sneaking, which each
//! vehicle's interaction checks. Mobs keep theirs in `extra.boarding_cooldown`
//! (`boat_passengers::BOARDING_COOLDOWN`).
use super::Game;
use sunrise_client::mobs::Mob;

/// `Entity.getIndirectPassengersStream` for `vehicle`: the vehicle and every
/// vehicle riding it, directly or through another rider. `mounts` holds
/// `(vehicle, its vehicle)` for every mob.
pub(super) fn passenger_vehicles(vehicle: u32, mounts: &[(u32, Option<u32>)]) -> Vec<u32> {
    let mut ids = vec![vehicle];
    let mut i = 0;
    while i < ids.len() {
        let v = ids[i];
        i += 1;
        for &(id, mount) in mounts {
            if mount == Some(v) && !ids.contains(&id) {
                ids.push(id);
            }
        }
    }
    ids
}

impl Game<'_> {
    /// `Entity.startRiding` for the acting player: the rider link, then
    /// `CriteriaTriggers.START_RIDING_TRIGGER` for every server player the
    /// vehicle carries.
    pub(super) fn start_riding(&mut self, vehicle: u32) {
        self.riding = Some(vehicle);
        self.seat_player(vehicle);
        self.note_started_riding(vehicle);
    }

    /// `ServerPlayer.startRiding` once the acting player is on `vehicle`: the vehicle seats it
    /// (`positionRider`), and its client hears that seat as a teleport and the vehicle's
    /// passengers at once, before the tracker's next look. `leave_guest` sends both.
    pub(in crate::game) fn tell_mounted_player(&mut self, vehicle: u32) {
        if self.lan_acting.is_none() {
            return;
        }
        let seat = if let Some(i) = self.boats.iter().position(|b| b.id == vehicle) {
            Some(self.player_boat_seat(i))
        } else if let Some(c) = self.carts.iter().find(|c| c.id == vehicle) {
            let at = c.position;
            Some(sunrise_core::geometry::Vec3::new(
                at.x,
                at.y + super::minecarts::SEAT,
                at.z,
            ))
        } else {
            if self.mobs.iter().any(|m| m.id == vehicle) {
                self.position_rider();
            }
            None
        };
        if let Some(p) = self.player.as_mut() {
            if let Some(at) = seat {
                p.position = at;
            }
            // `teleportSetPosition` with `Vec3.ZERO` and no relative motion.
            p.velocity = sunrise_core::geometry::Vec3::ZERO;
        }
        self.mounted = Some((vehicle, self.passenger_order(vehicle)));
    }

    /// The player riding through `riding` and its place: the acting guest (its kept place, if
    /// any, comes from the hub's list) or, outside a guest turn, this game's own player.
    pub(in crate::game) fn riding_player(&self) -> Option<(u32, i64)> {
        match (self.lan_acting_entity, self.lan_acting) {
            (Some(id), _) if self.lan_acting_uuid.is_some() => Some((id, i64::MIN)),
            (_, None) => self
                .player
                .as_ref()
                .map(|p| (p.entity_id as u32, self.riding_order)),
            _ => None,
        }
    }

    /// `Entity.passengers` of `vehicle`, first passenger first: each one's place, id and whether
    /// it is a player. Places tie only for those that never boarded through `addPassenger` (a
    /// load keeps its list order), players ahead.
    pub(in crate::game) fn seated_passengers(&self, vehicle: u32) -> Vec<(i64, u32, bool)> {
        let mut seats = Vec::new();
        self.any_passenger_seat(vehicle, &[], |seat| {
            seats.push(seat);
            false
        });
        seats.sort_by_key(|&(place, ..)| place);
        seats
    }

    /// The first seat, preserving enumeration order on a tie without sorting or allocating.
    pub(super) fn first_seated_passenger(&self, vehicle: u32) -> Option<(i64, u32, bool)> {
        let mut first: Option<(i64, u32, bool)> = None;
        self.any_passenger_seat(vehicle, &[], |seat| {
            if first.is_none_or(|old| seat.0 < old.0) {
                first = Some(seat);
            }
            false
        });
        first
    }

    /// `Entity.isVehicle`: any kind of rider makes this a vehicle.
    pub(in crate::game) fn has_seated_passengers(&self, vehicle: u32) -> bool {
        self.has_seated_passengers_with_lent_mobs(vehicle, &[])
    }

    /// The same check while the mob tick holds the resident list out of `Game`.
    pub(in crate::game) fn has_seated_passengers_with_lent_mobs(
        &self,
        vehicle: u32,
        lent_mobs: &[Mob],
    ) -> bool {
        self.any_passenger_seat(vehicle, lent_mobs, |_| true)
    }

    /// `getPassengers().size()` without allocating or sorting the seats.
    pub(in crate::game) fn seated_passenger_count(&self, vehicle: u32) -> usize {
        let mut count = 0;
        self.any_passenger_seat(vehicle, &[], |_| {
            count += 1;
            false
        });
        count
    }

    /// Shared seat enumeration; visit only until `matches` returns true. The
    /// enumeration order breaks ties for passengers with the same seat order.
    fn any_passenger_seat(
        &self,
        vehicle: u32,
        lent_mobs: &[Mob],
        mut matches: impl FnMut((i64, u32, bool)) -> bool,
    ) -> bool {
        let acting = self.lan_acting.filter(|_| self.player.is_some());
        let mut acting_listed = false;
        for g in self
            .lan
            .iter()
            .flat_map(|h| h.hub.guests.iter())
            .filter(|g| g.placed && g.dimension == self.dim.id())
        {
            let rides = if acting == Some(g.conn) {
                acting_listed = true;
                self.riding
            } else {
                g.vehicle
            };
            if rides == Some(vehicle) && matches((g.vehicle_order, g.entity_id as u32, true)) {
                return true;
            }
        }
        // An unlisted acting guest goes first. This game's own player keeps its seat order.
        if let Some((me, place)) = self.riding_player()
            && !acting_listed
            && self.player.is_some()
            && self.riding == Some(vehicle)
            && matches((place, me, true))
        {
            return true;
        }
        let other_mobs = lent_mobs
            .iter()
            .chain(
                self.persist
                    .off_level
                    .values()
                    .filter_map(super::persist::OffLevel::as_mob),
            )
            .filter(|m| m.extra.mount == Some(vehicle));
        self.resident_mob_riders(vehicle)
            .map(|(_, m)| m)
            .chain(other_mobs)
            .filter(|m| !m.removed)
            .any(|m| matches((m.extra.vehicle_order, m.id, false)))
            || self
                .general_passengers(vehicle)
                .any(|row| matches((row.vehicle_order, row.id, false)))
            || self
                .bolts
                .iter()
                .filter(|b| b.mount == Some(vehicle))
                .any(|b| matches((b.vehicle_order, b.id, false)))
    }
    /// `Entity.addPassenger`: the place of `rider` getting on `vehicle`. On a server a player
    /// goes ahead of a first passenger that is no player; everything else goes last.
    pub(in crate::game) fn boarding_order(&self, vehicle: u32, rider: u32, player: bool) -> i64 {
        let mut aboard = self.seated_passengers(vehicle);
        aboard.retain(|&(_, id, _)| id != rider);
        match (aboard.first(), aboard.last()) {
            (Some(&(first, _, false)), _) if player => first - 1,
            (_, Some(&(last, ..))) => last + 1,
            _ => 0,
        }
    }
    /// `addPassenger` for the mob `id`, linked to its vehicle just now.
    pub(in crate::game) fn seat_mob(&mut self, id: u32) {
        let mount = self.mob_object(id);
        let Some(vehicle) = mount.and_then(|m| m.extra.mount) else {
            return;
        };
        let order = self.boarding_order(vehicle, id, false);
        if let Some(m) = self.mob_object_mut(id) {
            m.extra.vehicle_order = order;
        }
    }
    /// `addPassenger` for the player riding through `riding`, now in `vehicle`: the acting
    /// guest's place goes to the hub, this game's own player's to `riding_order`.
    pub(in crate::game) fn seat_player(&mut self, vehicle: u32) {
        match (self.lan_acting, self.lan_acting_entity) {
            (Some(conn), Some(me)) => {
                let order = self.boarding_order(vehicle, me, true);
                if let Some(host) = self.lan.as_mut() {
                    host.hub.update_guest(conn, |g| g.vehicle_order = order);
                }
            }
            (None, _) => {
                if let Some((me, _)) = self.riding_player() {
                    self.riding_order = self.boarding_order(vehicle, me, true);
                }
            }
            _ => {}
        }
    }

    /// A rider link changed on `vehicle` (`sendEventAndTriggers`). Every
    /// player aboard hears the trigger; a host's turn can fire it now, while
    /// a guest's turn (the hub held by its action) leaves it for
    /// [`Self::flush_started_riding`] once every player is back in place.
    pub(super) fn note_started_riding(&mut self, vehicle: u32) {
        self.started_riding.push(vehicle);
        if self.lan.is_some() && self.lan_acting.is_none() {
            self.flush_started_riding();
        }
    }

    /// Fires the queued `started_riding` triggers with every player in place.
    pub(super) fn flush_started_riding(&mut self) {
        for vehicle in std::mem::take(&mut self.started_riding) {
            self.trigger_started_riding(vehicle);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `ServerPlayer.startRiding`: a guest put in a boat by `/ride` hears its seat as a teleport
    /// that keeps its look (`Relative.ROTATION`, rotation 0, no motion), then the boat's
    /// passengers, both in its own turn.
    #[test]
    #[ignore = "daily"]
    fn a_mounted_guest_hears_its_seat_then_the_passengers() {
        crate::game::native_guest_turn_tests::with_guests(|game| {
            use crate::game::commands::Target;
            use sunrise_protocol::packets::play::{Clientbound as C, relative};
            game.summon_with_nbt("minecraft:oak_boat", [1.0, 64.0, 0.5], "{}")
                .unwrap();
            game.lan_assign_ids();
            let boat = game.boats[0].id;
            let (tx, rx) = std::sync::mpsc::channel();
            let dimension = game.dim.id().to_owned();
            game.lan
                .as_mut()
                .unwrap()
                .hub
                .update_guest(13, |g| {
                    g.dimension = dimension;
                    g.sender = sunrise_client::lan::PlayerSender::new(tx, Default::default());
                })
                .unwrap();
            let mounted = game.as_guest(13, |game| {
                game.ride_mount(Target::Player, Target::Boat(boat)).is_ok()
            });
            assert_eq!(mounted, Some(true));
            let sent: Vec<_> = crate::game::native_sound_refs::tests::packets(&rx)
                .into_iter()
                .filter(|p| matches!(p, C::PlayerPosition(_) | C::SetPassengers(_)))
                .collect();
            let [C::PlayerPosition(seat), C::SetPassengers(aboard)] = &sent[..] else {
                panic!("a teleport then the passengers, got {sent:?}");
            };
            // The boat's ride height (0.5625 / 3) less the player's vehicle attachment (0.6).
            let expected = [1.0, 64.0 + 0.1875 - 0.6, 0.5];
            assert!(
                (0..3).all(|k| (seat.position[k] - expected[k]).abs() < 1e-9),
                "{:?}",
                seat.position
            );
            assert_eq!(seat.delta, [0.0; 3]);
            assert_eq!([seat.yaw, seat.pitch], [0.0, 0.0]);
            assert_eq!(seat.relatives, relative::Y_ROT | relative::X_ROT);
            assert_eq!(
                (aboard.vehicle, &aboard.passengers[..]),
                (boat as i32, &[13][..])
            );
        });
    }
    #[test]
    #[ignore = "daily"]
    fn retained_passengers_occupy_seats_in_boarding_order() {
        crate::game::native_entity_refs::tests::with_game(|game| {
            use sunrise_client::mobs::Mob;
            use sunrise_core::geometry::Vec3;
            game.build_registries();
            game.player = Some(
                sunrise_client::motion::PlayerMotion::new(game.motion, Vec3::new(8.5, 64.0, 8.5))
                    .unwrap(),
            );
            let pig = game.res.mob_types().get("minecraft:pig").unwrap().clone();
            for (id, order) in [(74, 2), (72, 0)] {
                let mut m = Mob::new(id, pig.clone(), Vec3::ZERO, 0.0);
                m.extra.mount = Some(71);
                m.extra.vehicle_order = order;
                game.persist
                    .off_level
                    .insert(id, crate::game::persist::OffLevel::Mob(m));
            }
            let mut resident = Mob::new(73, pig, Vec3::ZERO, 0.0);
            resident.extra.mount = Some(71);
            resident.extra.vehicle_order = 1;
            game.mobs.push(resident);
            assert_eq!(
                game.seated_passengers(71),
                vec![(0, 72, false), (1, 73, false), (2, 74, false)]
            );
            assert_eq!(game.boarding_order(71, 75, false), 3);
            assert_eq!(game.boarding_order(71, 75, true), -1);
            game.seat_mob(72);
            assert_eq!(game.mob_object(72).unwrap().extra.vehicle_order, 3);
        });
    }

    /// `Entity.addPassenger` appends a player when the first passenger is already one: this
    /// game's own player boarding after a guest and a pig sits third, not first.
    #[test]
    #[ignore = "daily"]
    fn the_own_player_boards_behind_a_guest_and_a_pig() {
        crate::game::native_guest_turn_tests::with_guests(|game| {
            use sunrise_client::mobs::Mob;
            use sunrise_core::geometry::Vec3;
            game.build_registries();
            game.player = Some(
                sunrise_client::motion::PlayerMotion::new(game.motion, Vec3::new(8.5, 64.0, 8.5))
                    .unwrap(),
            );
            let pig = game.res.mob_types().get("minecraft:pig").unwrap().clone();
            let (vehicle, rider) = (90, 91);
            game.mobs
                .push(Mob::new(vehicle, pig.clone(), Vec3::ZERO, 0.0));
            let order = game.boarding_order(vehicle, 12, true);
            let dimension = game.dim.id().to_owned();
            game.lan
                .as_mut()
                .unwrap()
                .hub
                .update_guest(12, |g| {
                    g.dimension = dimension;
                    g.vehicle = Some(vehicle);
                    g.vehicle_order = order;
                })
                .unwrap();
            let mut pig = Mob::new(rider, pig, Vec3::ZERO, 0.0);
            pig.extra.mount = Some(vehicle);
            game.mobs.push(pig);
            game.seat_mob(rider);
            game.start_riding(vehicle);
            let own = game.player.as_ref().unwrap().entity_id as u32;
            let aboard: Vec<u32> = game
                .seated_passengers(vehicle)
                .into_iter()
                .map(|(_, id, _)| id)
                .collect();
            assert_eq!(aboard, vec![12, rider, own]);
        });
    }

    /// `Entity.positionRider` picks the attachment by the passenger's index in `getPassengers`: the
    /// own player behind a guest and a pig sits in the third seat of a happy ghast, 1.7 south of it.
    #[test]
    #[ignore = "daily"]
    fn the_own_player_in_the_third_seat_of_a_happy_ghast_sits_there() {
        crate::game::native_guest_turn_tests::with_guests(|game| {
            use sunrise_client::mobs::Mob;
            use sunrise_core::geometry::Vec3;
            game.build_registries();
            game.player = Some(
                sunrise_client::motion::PlayerMotion::new(game.motion, Vec3::new(8.5, 64.0, 8.5))
                    .unwrap(),
            );
            let ghast = game
                .res
                .mob_types()
                .get("minecraft:happy_ghast")
                .unwrap()
                .clone();
            let pig = game.res.mob_types().get("minecraft:pig").unwrap().clone();
            let (vehicle, rider) = (90, 91);
            game.mobs
                .push(Mob::new(vehicle, ghast, Vec3::new(8.5, 64.0, 8.5), 0.0));
            let order = game.boarding_order(vehicle, 12, true);
            let dimension = game.dim.id().to_owned();
            game.lan
                .as_mut()
                .unwrap()
                .hub
                .update_guest(12, |g| {
                    g.dimension = dimension;
                    g.vehicle = Some(vehicle);
                    g.vehicle_order = order;
                })
                .unwrap();
            let mut pig = Mob::new(rider, pig, Vec3::ZERO, 0.0);
            pig.extra.mount = Some(vehicle);
            game.mobs.push(pig);
            game.seat_mob(rider);
            game.start_riding(vehicle);
            game.position_rider();
            let at = game.player.as_ref().unwrap().position;
            assert_eq!(
                (at.x, at.z),
                (8.5, 8.5 - 1.7),
                "the third seat, not the first"
            );
        });
    }

    #[test]
    fn started_riding_reaches_every_rider_of_the_vehicle() {
        // Entity.startRiding: START_RIDING_TRIGGER for every ServerPlayer in
        // entityToRide.getIndirectPassengersStream(). A mob between two
        // riders is still part of that stream, and another vehicle is not.
        let mounts = [(7, Some(5)), (8, Some(7)), (9, Some(4))];
        let aboard = passenger_vehicles(5, &mounts);
        assert!(aboard.contains(&5), "the rider on the vehicle itself");
        assert!(aboard.contains(&7), "the mob in the vehicle's seat");
        assert!(aboard.contains(&8), "the rider of that mob");
        assert!(!aboard.contains(&9), "a mob riding another vehicle");
        assert!(!aboard.contains(&6), "a mob riding nothing");
    }
}
