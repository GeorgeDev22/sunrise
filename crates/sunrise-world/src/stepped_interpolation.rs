//! How a client moves a living entity it doesn't simulate along the
//! positions its server sends (vanilla `SteppedInterpolationHandler`, which
//! `LivingEntity.createInterpolationHandler` gives every living entity on a
//! client): each position is a step the entity walks to in the type's update
//! interval (3 ticks for most mobs), one after the other, sped up while
//! steps pile up. A punched mob's first move, sent at once (`needsSync`),
//! thus takes three ticks to show, and the rest follow it.

use std::collections::VecDeque;

/// A position and rotation (`PositionAndRotation`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PosRot {
    pub position: [f64; 3],
    pub yaw: f32,
    pub pitch: f32,
}

impl PosRot {
    /// `Entity.setRot`, as `setPos`/`setRot` place an entity at a step: the
    /// yaw kept within a turn (`yRot % 360.0F`), the pitch too, both through
    /// `setYRot` and `setXRot`.
    pub fn set_rot(&mut self, yaw: f32, pitch: f32) {
        self.set_y_rot(yaw % 360.0);
        self.set_x_rot(pitch % 360.0);
    }
    /// `Entity.setYRot` (`snapTo` calls it with the packet's yaw as it is):
    /// a yaw that isn't finite is discarded, keeping the old one.
    pub fn set_y_rot(&mut self, yaw: f32) {
        if yaw.is_finite() {
            self.yaw = yaw;
        }
    }
    /// `Entity.setXRot`: the pitch within a turn and clamped to ±90; one that
    /// isn't finite is discarded.
    pub fn set_x_rot(&mut self, pitch: f32) {
        if pitch.is_finite() {
            self.pitch = (pitch % 360.0).clamp(-90.0, 90.0);
        }
    }
    /// `PositionAndRotation.is`: the same rotation and `Vec3.equals`
    /// position (`Double.compare`, so -0.0 is not 0.0).
    fn is(&self, position: [f64; 3], yaw: f32, pitch: f32) -> bool {
        self.yaw == yaw && self.pitch == pitch && same_position(self.position, position)
    }
}

/// `Vec3.equals`.
fn same_position(a: [f64; 3], b: [f64; 3]) -> bool {
    (0..3).all(|i| a[i].to_bits() == b[i].to_bits())
}

/// `PositionPath`: where a move packet puts an entity, either straight
/// there or through the positions the server stepped it through, each
/// with the ticks it took.
#[derive(Clone, Debug, PartialEq)]
pub enum Path {
    Linear([f64; 3]),
    Stepped(Vec<([f64; 3], i32)>),
}

impl Path {
    pub fn end(&self) -> [f64; 3] {
        match self {
            Path::Linear(p) => *p,
            Path::Stepped(steps) => steps.last().map_or([0.0; 3], |s| s.0),
        }
    }
}

/// What the client does with the entity when a packet arrives.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Start {
    /// Moves on to it over the coming ticks.
    Interpolating,
    /// Already headed there: nothing changes.
    Unchanged,
    /// Is put there at once (`snapTo`): types with no update interval.
    Snap(PosRot),
}

/// `SteppedInterpolationHandler` and its `InterpolationData`.
#[derive(Clone, Debug)]
pub struct SteppedInterpolation {
    /// `AbstractInterpolationHandler.interpolationSteps`: the type's
    /// `updateInterval`.
    steps: i32,
    /// The data's own position and rotation: where the last packet said.
    target: PosRot,
    /// `lastStepPosRot`: where the step being walked started.
    last: PosRot,
    remaining: VecDeque<(PosRot, i32)>,
    current_step_ticks: f32,
    remaining_ticks: f32,
    speed: f32,
}

/// `Mth.wrapDegrees(float)`.
fn wrap_degrees(degrees: f32) -> f32 {
    let mut wrapped = degrees % 360.0;
    if wrapped >= 180.0 {
        wrapped -= 360.0;
    }
    if wrapped < -180.0 {
        wrapped += 360.0;
    }
    wrapped
}

/// `Mth.lerp(float, float, float)`.
fn lerp(a: f32, from: f32, to: f32) -> f32 {
    from + a * (to - from)
}

/// `Mth.rotLerp`.
fn rot_lerp(a: f32, from: f32, to: f32) -> f32 {
    from + a * wrap_degrees(to - from)
}

impl SteppedInterpolation {
    /// The handler of an entity type whose update interval is `steps` ticks.
    pub fn new(steps: i32) -> Self {
        Self {
            steps,
            target: PosRot::default(),
            last: PosRot::default(),
            remaining: VecDeque::new(),
            current_step_ticks: 0.0,
            remaining_ticks: 0.0,
            speed: 1.0,
        }
    }
    /// `hasActiveInterpolation`.
    pub fn active(&self) -> bool {
        !self.remaining.is_empty()
    }
    /// Where the entity is headed: the last packet's position and rotation
    /// while interpolating (`AbstractInterpolationHandler.interpolateTo`'s
    /// `current`), else where it is.
    pub fn heading(&self, entity: PosRot) -> PosRot {
        if self.active() { self.target } else { entity }
    }
    /// `AbstractInterpolationHandler.interpolateTo(path, yRot, xRot,
    /// hasRotation)` for an entity at `entity`: a packet without a position
    /// (`path` None) keeps the one it is headed for, one without a rotation
    /// the rotation.
    pub fn interpolate_to(
        &mut self,
        entity: PosRot,
        path: Option<Path>,
        rotation: Option<(f32, f32)>,
    ) -> Start {
        let current = self.heading(entity);
        let path = path.unwrap_or(Path::Linear(current.position));
        let (yaw, pitch) = rotation.unwrap_or((current.yaw, current.pitch));
        let end = path.end();
        if self.steps == 0 {
            self.cancel();
            return Start::Snap(PosRot {
                position: end,
                yaw,
                pitch,
            });
        }
        if self.active() && self.target.is(end, yaw, pitch) {
            return Start::Unchanged;
        }
        // SteppedInterpolationHandler.startInterpolating.
        if !self.active() {
            self.last = entity;
            self.current_step_ticks = 1.0;
        }
        if same_position(end, self.target.position) {
            self.add_step(
                PosRot {
                    position: end,
                    yaw,
                    pitch,
                },
                self.steps,
            );
        } else {
            self.add_steps(&path, yaw, pitch);
        }
        self.target = PosRot {
            position: end,
            yaw,
            pitch,
        };
        Start::Interpolating
    }
    /// `cancel` (`InterpolationData.reset`).
    pub fn cancel(&mut self) {
        self.remaining.clear();
        self.remaining_ticks = 0.0;
        self.speed = 1.0;
    }
    /// `AbstractInterpolationHandler.interpolate`, once a client tick: where
    /// the entity is put, or None when there is nothing to walk (and the
    /// handler resets). The client moves the entity only this way, so the
    /// deltas `adjustInterpolationTargetFromDeltas` adds are none for the
    /// position and +0.0 for the rotation.
    pub fn interpolate(&mut self) -> Option<PosRot> {
        if !self.active() {
            self.cancel();
            return None;
        }
        // `addRotation(0.0F, 0.0F)`: no turn, but a -0.0 angle becomes 0.0.
        self.add_rotation(0.0, 0.0);
        // doInterpolate: getNewPositionAndRotation, then advance by the
        // level's relative tick speed (1 at the normal tick rate).
        let at = self.new_position();
        self.advance(1.0);
        Some(at)
    }
    /// `InterpolationData.addRotation`: the target, every step left and the
    /// one being walked from.
    fn add_rotation(&mut self, yaw: f32, pitch: f32) {
        let steps = self.remaining.iter_mut().map(|(step, _)| step);
        for r in [&mut self.target, &mut self.last].into_iter().chain(steps) {
            r.yaw += yaw;
            r.pitch += pitch;
        }
    }
    /// `InterpolationData.addStep`.
    fn add_step(&mut self, step: PosRot, ticks: i32) {
        self.remaining.push_back((step, ticks));
        self.remaining_ticks += ticks as f32;
    }
    /// `InterpolationData.addSteps`: a stepped path's steps take their own
    /// ticks, turning by the share of the path each one ends.
    fn add_steps(&mut self, path: &Path, yaw: f32, pitch: f32) {
        match path {
            Path::Linear(p) => self.add_step(
                PosRot {
                    position: *p,
                    yaw,
                    pitch,
                },
                self.steps,
            ),
            Path::Stepped(steps) => {
                if yaw == self.target.yaw && pitch == self.target.pitch {
                    for (p, ticks) in steps {
                        self.add_step(
                            PosRot {
                                position: *p,
                                yaw,
                                pitch,
                            },
                            *ticks,
                        );
                    }
                    return;
                }
                let total: i32 = steps.iter().map(|s| s.1).sum();
                let mut offset = 0;
                let (from_yaw, from_pitch) = (self.target.yaw, self.target.pitch);
                for (p, ticks) in steps {
                    offset += ticks;
                    let a = offset as f32 / total as f32;
                    let step = PosRot {
                        position: *p,
                        yaw: rot_lerp(a, from_yaw, yaw),
                        pitch: lerp(a, from_pitch, pitch),
                    };
                    self.add_step(step, *ticks);
                }
            }
        }
    }
    /// `InterpolationData.getNewPositionAndRotation`.
    fn new_position(&mut self) -> PosRot {
        while let Some(&(step, ticks)) = self.remaining.front() {
            if self.current_step_ticks < ticks as f32 {
                let a = self.current_step_ticks / ticks as f32;
                let t = f64::from(a);
                let (from, to) = (self.last.position, step.position);
                return PosRot {
                    position: [0, 1, 2].map(|i| from[i] + (to[i] - from[i]) * t),
                    yaw: rot_lerp(a, self.last.yaw, step.yaw),
                    pitch: lerp(a, self.last.pitch, step.pitch),
                };
            }
            self.current_step_ticks -= ticks as f32;
            self.last = step;
            self.remaining.pop_front();
        }
        self.target
    }
    /// `InterpolationData.advance`: faster while more than one update's
    /// worth of steps waits.
    fn advance(&mut self, ticks: f32) {
        let steps = self.steps as f32;
        let target_speed = (self.remaining_ticks / steps).max(1.0);
        self.speed = lerp(1.0 / steps, self.speed, target_speed);
        let moved = if ticks * self.speed < self.remaining_ticks {
            ticks * self.speed
        } else {
            self.speed = 1.0;
            self.remaining_ticks
        };
        self.current_step_ticks += moved;
        self.remaining_ticks -= moved;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(x: f64) -> PosRot {
        PosRot {
            position: [x, 0.0, 0.0],
            yaw: 0.0,
            pitch: 0.0,
        }
    }

    #[test]
    fn a_position_takes_the_update_interval_to_reach() {
        let mut h = SteppedInterpolation::new(3);
        let mut entity = at(0.0);
        h.interpolate_to(entity, Some(Path::Linear([0.3, 0.0, 0.0])), None);
        let mut seen = Vec::new();
        while let Some(p) = h.interpolate() {
            entity = p;
            seen.push((p.position[0] * 1000.0).round() / 1000.0);
        }
        assert_eq!(seen, [0.1, 0.2, 0.3]);
        assert!(!h.active());
        assert_eq!(entity.position[0], 0.3);
    }

    #[test]
    fn steps_that_pile_up_are_walked_faster() {
        let mut h = SteppedInterpolation::new(3);
        let mut entity = at(0.0);
        let mut ticks = 0;
        for k in 1..=4 {
            h.interpolate_to(entity, Some(Path::Linear([f64::from(k), 0.0, 0.0])), None);
            if let Some(p) = h.interpolate() {
                entity = p;
            }
            ticks += 1;
        }
        while let Some(p) = h.interpolate() {
            entity = p;
            ticks += 1;
        }
        // Four packets a tick apart: 12 ticks of steps, walked in fewer.
        assert!(ticks < 12, "{ticks}");
        assert_eq!(entity.position[0], 4.0);
    }

    #[test]
    fn a_step_keeps_the_yaw_within_a_turn_and_a_snap_does_not() {
        let mut r = PosRot::default();
        r.set_rot(570.0, 100.0);
        assert_eq!((r.yaw, r.pitch), (210.0, 90.0));
        r.set_rot(-490.0, f32::NAN);
        assert_eq!((r.yaw, r.pitch), (-130.0, 90.0));
        r.set_rot(f32::INFINITY, -400.0);
        assert_eq!((r.yaw, r.pitch), (-130.0, -40.0));
        r.set_y_rot(570.0);
        r.set_x_rot(f32::NEG_INFINITY);
        assert_eq!((r.yaw, r.pitch), (570.0, -40.0));
    }

    #[test]
    fn no_update_interval_snaps() {
        let mut h = SteppedInterpolation::new(0);
        let start = h.interpolate_to(
            at(0.0),
            Some(Path::Linear([2.0, 0.0, 0.0])),
            Some((90.0, 0.0)),
        );
        assert_eq!(
            start,
            Start::Snap(PosRot {
                position: [2.0, 0.0, 0.0],
                yaw: 90.0,
                pitch: 0.0
            })
        );
    }
}
