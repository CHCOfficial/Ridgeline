//! A swept fixed-step sphere controller tailored to a single continuous heightfield.
//!
//! Compared with triangle-mesh rigid bodies, this preserves a perfectly smooth contact normal
//! across streamed chunk borders, avoids collider rebuild stalls, and lets visible geometry and
//! collision share the exact same function. Velocity is never assigned from input: player intent
//! contributes force-like acceleration and naturally competes with gravity and momentum.

use crate::{config, persistence::BallFeel, terrain::TerrainField};
use glam::{Quat, Vec2, Vec3};

#[derive(Clone, Copy, Debug, Default)]
pub struct ControlIntent {
    pub direction: Vec3,
    pub strength: f32,
    pub jump: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PhysicsSignal {
    None,
    Recovered,
    Fell,
}

#[derive(Clone, Copy, Debug)]
struct BallTuning {
    drive: f32,
    air_control: f32,
    steering_loss: f32,
    rolling_drag: f32,
    top_speed: f32,
}

impl BallTuning {
    fn for_feel(feel: BallFeel) -> Self {
        match feel {
            BallFeel::Precision => Self {
                drive: 0.92,
                air_control: 1.08,
                steering_loss: 0.43,
                rolling_drag: 1.26,
                top_speed: 0.87,
            },
            BallFeel::Responsive => Self {
                drive: 1.22,
                air_control: 1.10,
                steering_loss: 0.57,
                rolling_drag: 0.78,
                top_speed: 1.0,
            },
            BallFeel::Momentum => Self {
                drive: 1.05,
                air_control: 0.90,
                steering_loss: 0.72,
                rolling_drag: 0.56,
                top_speed: 1.14,
            },
        }
    }
}

pub struct SphereBody {
    pub position: Vec3,
    pub previous_position: Vec3,
    pub velocity: Vec3,
    pub rotation: Quat,
    pub previous_rotation: Quat,
    pub grounded: bool,
    pub contact_normal: Vec3,
    pub pulse: f32,
    last_safe_position: Vec3,
    stuck_time: f32,
    recovery_cooldown: f32,
    tear_warning_time: f32,
    tear_armed: bool,
}

impl SphereBody {
    pub fn new(field: TerrainField) -> Self {
        let (floor, contact_normal, _) = sphere_surface_contact(field, 0.0, 0.0);
        let start = Vec3::new(0.0, floor, 0.0);
        Self {
            position: start,
            previous_position: start,
            velocity: Vec3::new(0.0, 0.0, 1.8),
            rotation: Quat::IDENTITY,
            previous_rotation: Quat::IDENTITY,
            grounded: true,
            contact_normal,
            pulse: 0.0,
            last_safe_position: start,
            stuck_time: 0.0,
            recovery_cooldown: 0.0,
            tear_warning_time: 0.0,
            tear_armed: false,
        }
    }

    pub fn speed(&self) -> f32 {
        Vec2::new(self.velocity.x, self.velocity.z).length()
    }

    pub fn reproject_to_surface(&mut self, field: TerrainField) {
        let (floor, normal, _) = sphere_surface_contact(field, self.position.x, self.position.z);
        self.position.y = floor;
        self.previous_position = self.position;
        self.velocity.y = 0.0;
        self.grounded = true;
        self.contact_normal = normal;
        self.last_safe_position = self.position;
        self.tear_warning_time = 0.0;
        self.tear_armed = false;
    }

    pub fn fixed_step(
        &mut self,
        field: TerrainField,
        intent: ControlIntent,
        feel: BallFeel,
        difficulty: f32,
        hazards_rendered: bool,
        dt: f32,
    ) -> PhysicsSignal {
        self.previous_position = self.position;
        self.previous_rotation = self.rotation;
        self.recovery_cooldown = (self.recovery_cooldown - dt).max(0.0);
        self.pulse = (self.pulse - dt * 5.5).max(0.0);

        let visible_overlap = hazards_rendered
            && field.ball_overlaps_visible_tear(
                self.position.x,
                self.position.z,
                config::BALL_RADIUS,
            );
        if !visible_overlap {
            self.tear_warning_time = 0.0;
            self.tear_armed = false;
        } else if self.grounded && !self.tear_armed {
            self.tear_warning_time += dt;
            self.tear_armed = self.tear_warning_time >= config::TEAR_WARNING_ARM_TIME;
        }
        let tear_armed = self.tear_armed;

        if intent.jump && self.grounded {
            let launch = (Vec3::Y * 0.88 + self.contact_normal * 0.12).normalize_or_zero();
            self.velocity += launch * config::JUMP_SPEED;
            self.grounded = false;
            self.stuck_time = 0.0;
        }

        let tuning = BallTuning::for_feel(feel);
        let speed = self.speed();
        let max_speed = ((config::MAX_SPEED_BASE + difficulty * 10.0) * tuning.top_speed)
            .min(config::MAX_SPEED_CAP * tuning.top_speed);
        let steering_falloff = 1.0 - tuning.steering_loss * (speed / max_speed).clamp(0.0, 1.0);
        let mut acceleration = Vec3::new(0.0, -config::GRAVITY, 0.0);

        if intent.strength > 0.01 {
            if self.grounded {
                let tangent = (intent.direction
                    - self.contact_normal * intent.direction.dot(self.contact_normal))
                .normalize_or_zero();
                acceleration += tangent
                    * config::GROUND_ACCELERATION
                    * tuning.drive
                    * intent.strength
                    * steering_falloff;
            } else {
                acceleration += intent.direction
                    * config::AIR_ACCELERATION
                    * tuning.air_control
                    * intent.strength;
            }
        }

        if self.grounded {
            let tangent_velocity =
                self.velocity - self.contact_normal * self.velocity.dot(self.contact_normal);
            acceleration -= tangent_velocity * config::ROLLING_RESISTANCE * tuning.rolling_drag;
        }

        self.velocity += acceleration * dt;
        let horizontal = Vec2::new(self.velocity.x, self.velocity.z);
        if horizontal.length() > max_speed {
            let limited = horizontal.normalize() * max_speed;
            self.velocity.x = limited.x;
            self.velocity.z = limited.y;
        }

        // Two half steps act as a cheap sweep and prevent high-speed ridge tunnelling.
        self.integrate_contact(field, tear_armed, dt * 0.5);
        self.integrate_contact(field, tear_armed, dt * 0.5);

        let travelled = Vec2::new(
            self.position.x - self.previous_position.x,
            self.position.z - self.previous_position.z,
        );
        if travelled.length_squared() > 0.0 {
            let axis = Vec3::new(travelled.y, 0.0, -travelled.x).normalize_or_zero();
            let angle = travelled.length() / config::BALL_RADIUS;
            self.rotation = Quat::from_axis_angle(axis, angle) * self.rotation;
        }

        let terrain_height = field.height(self.position.x, self.position.z);
        let lethal_tear = tear_armed
            && field.ball_overlaps_visible_tear(
                self.position.x,
                self.position.z,
                config::BALL_RADIUS,
            );
        if lethal_tear && self.position.y < terrain_height - 6.0 {
            return PhysicsSignal::Fell;
        }
        // Steep terrain can briefly outrun the heightfield contact solver. That must never look
        // like an invisible hole: only a renderer-verified tear may end a run. Recover any deep
        // non-tear penetration to the last known safe surface instead.
        if !lethal_tear && self.position.y < terrain_height - 14.0 {
            self.position = self.last_safe_position + Vec3::Y * 1.2;
            self.previous_position = self.position;
            self.velocity = Vec3::new(0.0, 1.0, 3.0);
            self.grounded = false;
            self.recovery_cooldown = 18.0;
            self.tear_warning_time = 0.0;
            self.tear_armed = false;
            return PhysicsSignal::Recovered;
        }

        let slope = 1.0 - self.contact_normal.y;
        if self.grounded
            && slope < 0.22
            && self.speed() > 2.0
            && !field.ball_overlaps_visible_tear(
                self.position.x,
                self.position.z,
                config::BALL_RADIUS,
            )
        {
            self.last_safe_position = self.position;
        }
        if self.grounded && self.speed() < 0.42 && intent.strength > 0.4 {
            self.stuck_time += dt;
        } else {
            self.stuck_time = (self.stuck_time - dt * 1.8).max(0.0);
        }

        if self.stuck_time > 6.5 {
            self.stuck_time = 0.0;
            self.position = self.last_safe_position + Vec3::Y * 1.2;
            self.previous_position = self.position;
            self.velocity = Vec3::new(0.0, 1.0, 3.0);
            self.recovery_cooldown = 18.0;
            self.tear_warning_time = 0.0;
            self.tear_armed = false;
            return PhysicsSignal::Recovered;
        }
        PhysicsSignal::None
    }

    fn integrate_contact(&mut self, field: TerrainField, tear_armed: bool, dt: f32) {
        self.position += self.velocity * dt;
        if tear_armed
            && field.ball_overlaps_visible_tear(
                self.position.x,
                self.position.z,
                config::BALL_RADIUS,
            )
        {
            self.grounded = false;
            self.contact_normal = Vec3::Y;
            return;
        }
        let (floor, normal, _) = sphere_surface_contact(field, self.position.x, self.position.z);
        let penetrating_surface = self.position.y < floor;
        let landing = self.position.y <= floor + config::GROUND_SNAP && self.velocity.y <= 2.4;
        if penetrating_surface || landing {
            self.position.y = floor;
            let into_surface = self.velocity.dot(normal);
            if into_surface < 0.0 {
                let impact = -into_surface;
                let restitution = if impact > 9.0 { 0.16 } else { 0.02 };
                self.velocity -= normal * into_surface * (1.0 + restitution);
            }
            self.grounded = true;
            self.contact_normal = normal;
        } else {
            self.grounded = false;
            self.contact_normal = normal;
        }
    }
}

/// Finds the height at which the sphere is tangent to the local surface rather than merely
/// placing its centre one radius above the height at its centre. The fixed-point iterations find
/// the laterally offset contact point on a slope. The conservative vertical bound and tiny visual
/// margin cover interpolation error in the rendered triangle mesh, so the lower hemisphere never
/// appears to cut through the grid.
fn sphere_surface_contact(field: TerrainField, center_x: f32, center_z: f32) -> (f32, Vec3, Vec2) {
    let center = Vec2::new(center_x, center_z);
    let mut contact = center;
    let mut normal = field.normal(contact.x, contact.y);
    for _ in 0..8 {
        contact = center - Vec2::new(normal.x, normal.z) * config::BALL_RADIUS;
        normal = field.normal(contact.x, contact.y);
    }
    let tangent_floor = field.height(contact.x, contact.y)
        + normal.y * config::BALL_RADIUS
        + config::BALL_SURFACE_CLEARANCE / normal.y.max(0.25);
    let vertical_floor =
        field.height(center_x, center_z) + config::BALL_RADIUS + config::BALL_SURFACE_CLEARANCE;
    let mut floor = tangent_floor.max(vertical_floor);
    let contact_surface = Vec3::new(contact.x, field.height(contact.x, contact.y), contact.y);
    let provisional_center = Vec3::new(center_x, floor, center_z);
    let clearance = (provisional_center - contact_surface).dot(normal);
    let required = config::BALL_RADIUS + config::BALL_SURFACE_CLEARANCE;
    if clearance < required {
        floor += (required - clearance) / normal.y.max(0.25);
    }
    (floor, normal, contact)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ball_feel_presets_have_distinct_handling_profiles() {
        let precision = BallTuning::for_feel(BallFeel::Precision);
        let responsive = BallTuning::for_feel(BallFeel::Responsive);
        let momentum = BallTuning::for_feel(BallFeel::Momentum);

        assert!(responsive.drive > 1.0);
        assert!(responsive.drive > precision.drive);
        assert!(precision.rolling_drag > responsive.rolling_drag);
        assert!(momentum.rolling_drag < responsive.rolling_drag);
        assert!(momentum.top_speed > responsive.top_speed);
    }

    #[test]
    fn sphere_is_clear_of_steep_surface_contact() {
        let field = TerrainField::new(0xC11F_F1E1D, 2.2);
        let mut steepest = (0.0, 0.0, 0.0);
        for z in (-300..=300).step_by(12) {
            for x in (-300..=300).step_by(12) {
                let x = x as f32;
                let z = z as f32;
                let slope = field.slope(x, z);
                if slope > steepest.2 {
                    steepest = (x, z, slope);
                }
                assert_surface_clearance(field, x, z);
            }
        }
        assert!(
            steepest.2 > 0.1,
            "test seed should contain a meaningful slope"
        );
        assert_surface_clearance(field, steepest.0, steepest.1);
    }

    #[test]
    fn renderer_confirmed_tear_requires_warning_time_before_falling() {
        let field = TerrainField::new(0x7EA2_5AFE, config::TERRAIN_INTENSITY_DEFAULT);
        let tear = (-500..=500)
            .step_by(2)
            .flat_map(|z| (-500..=500).step_by(2).map(move |x| (x, z)))
            .find(|&(x, z)| field.is_tear(x as f32, z as f32))
            .expect("test region should contain a tear");
        let (floor, normal, _) = sphere_surface_contact(field, tear.0 as f32, tear.1 as f32);
        let mut ball = SphereBody::new(field);
        ball.position = Vec3::new(tear.0 as f32, floor, tear.1 as f32);
        ball.previous_position = ball.position;
        ball.velocity = Vec3::ZERO;
        ball.contact_normal = normal;
        ball.grounded = true;

        let mut signal = PhysicsSignal::None;
        let safe_steps = (config::TEAR_WARNING_ARM_TIME / config::FIXED_DT).floor() as usize;
        for _ in 0..safe_steps {
            signal = ball.fixed_step(
                field,
                ControlIntent::default(),
                BallFeel::Responsive,
                0.0,
                true,
                config::FIXED_DT,
            );
            assert_ne!(signal, PhysicsSignal::Fell);
        }
        for _ in 0..240 {
            signal = ball.fixed_step(
                field,
                ControlIntent::default(),
                BallFeel::Responsive,
                0.0,
                true,
                config::FIXED_DT,
            );
            if signal == PhysicsSignal::Fell {
                break;
            }
        }
        assert_eq!(signal, PhysicsSignal::Fell);
    }

    #[test]
    fn deep_non_tear_penetration_is_resolved_as_solid_contact() {
        let field = TerrainField::new(0xBCE1_3BB2_45B4_7C2E, config::TERRAIN_INTENSITY_DEFAULT);
        let mut ball = SphereBody::new(field);
        ball.position = Vec3::new(24.0, field.height(24.0, 24.0) - 20.0, 24.0);
        ball.previous_position = ball.position;
        ball.velocity = Vec3::ZERO;
        ball.grounded = false;

        let signal = ball.fixed_step(
            field,
            ControlIntent::default(),
            BallFeel::Responsive,
            0.0,
            true,
            config::FIXED_DT,
        );

        assert_eq!(signal, PhysicsSignal::None);
        assert!(!field.is_tear(ball.position.x, ball.position.z));
        assert!(ball.grounded);
        let (floor, _, _) = sphere_surface_contact(field, ball.position.x, ball.position.z);
        assert!((ball.position.y - floor).abs() < 0.001);
    }

    #[test]
    fn unrendered_tear_is_always_solid_and_cannot_end_a_run() {
        let field = TerrainField::new(0xBDDA_F1A5_038B_DB5F, config::TERRAIN_INTENSITY_DEFAULT);
        let tear = (-700..=700)
            .step_by(2)
            .flat_map(|z| (-700..=700).step_by(2).map(move |x| (x, z)))
            .find(|&(x, z)| field.is_tear(x as f32, z as f32))
            .expect("reported seed should contain a renderer-verifiable tear");
        let (floor, normal, _) = sphere_surface_contact(field, tear.0 as f32, tear.1 as f32);
        let mut ball = SphereBody::new(field);
        ball.position = Vec3::new(tear.0 as f32, floor, tear.1 as f32);
        ball.previous_position = ball.position;
        ball.velocity = Vec3::ZERO;
        ball.contact_normal = normal;
        ball.grounded = true;

        for _ in 0..1_200 {
            let signal = ball.fixed_step(
                field,
                ControlIntent::default(),
                BallFeel::Responsive,
                0.0,
                false,
                config::FIXED_DT,
            );
            assert_ne!(signal, PhysicsSignal::Fell);
        }
        assert!(ball.grounded);
    }

    #[test]
    fn airborne_or_subsurface_entry_cannot_arm_an_unseen_tear() {
        let field = TerrainField::new(0xBDDA_F1A5_038B_DB5F, config::TERRAIN_INTENSITY_DEFAULT);
        let tear = (-700..=700)
            .step_by(2)
            .flat_map(|z| (-700..=700).step_by(2).map(move |x| (x, z)))
            .find(|&(x, z)| field.is_tear(x as f32, z as f32))
            .expect("reported seed should contain a renderer-verifiable tear");
        let mut ball = SphereBody::new(field);
        ball.position = Vec3::new(
            tear.0 as f32,
            field.height(tear.0 as f32, tear.1 as f32) - 2.0,
            tear.1 as f32,
        );
        ball.previous_position = ball.position;
        ball.velocity = Vec3::ZERO;
        ball.grounded = false;

        let signal = ball.fixed_step(
            field,
            ControlIntent::default(),
            BallFeel::Responsive,
            0.0,
            true,
            config::FIXED_DT,
        );
        assert_ne!(signal, PhysicsSignal::Fell);
        assert!(ball.grounded);
        assert!(!ball.tear_armed);
    }

    #[test]
    fn ball_footprint_detects_and_falls_through_a_visible_tear_edge() {
        let field = TerrainField::new(0xBB43_28BA_B873_A1F7, config::TERRAIN_INTENSITY_DEFAULT);
        let tear = (-900..=900)
            .step_by(2)
            .flat_map(|z| {
                (-900..=900)
                    .step_by(2)
                    .map(move |x| Vec2::new(x as f32, z as f32))
            })
            .find(|point| field.is_tear(point.x, point.y))
            .expect("reported seed should contain a renderer-verifiable tear");
        let edge = (-24..=24)
            .flat_map(|z| (-24..=24).map(move |x| Vec2::new(x as f32, z as f32) * 0.10))
            .map(|offset| tear + offset)
            .find(|point| {
                !field.is_tear(point.x, point.y)
                    && field.ball_overlaps_visible_tear(point.x, point.y, config::BALL_RADIUS)
            })
            .expect("ball footprint should detect a visible edge beyond the old point core");
        let (floor, normal, _) = sphere_surface_contact(field, edge.x, edge.y);
        let mut ball = SphereBody::new(field);
        ball.position = Vec3::new(edge.x, floor, edge.y);
        ball.previous_position = ball.position;
        ball.velocity = Vec3::ZERO;
        ball.contact_normal = normal;
        ball.grounded = true;

        let mut signal = PhysicsSignal::None;
        for _ in 0..240 {
            signal = ball.fixed_step(
                field,
                ControlIntent::default(),
                BallFeel::Responsive,
                0.0,
                true,
                config::FIXED_DT,
            );
            if signal == PhysicsSignal::Fell {
                break;
            }
        }
        assert_eq!(signal, PhysicsSignal::Fell);
    }

    #[test]
    fn reported_seed_long_run_cannot_fall_without_a_rendered_hazard() {
        let field = TerrainField::new(0xBDDA_F1A5_038B_DB5F, config::TERRAIN_INTENSITY_DEFAULT);
        let mut ball = SphereBody::new(field);
        let mut distance = 0.0;

        for step in 0..(120 * 180) {
            let phase = step as f32 * config::FIXED_DT;
            let direction = Vec3::new((phase * 0.17).sin() * 0.22, 0.0, 1.0).normalize();
            let signal = ball.fixed_step(
                field,
                ControlIntent {
                    direction,
                    strength: 1.0,
                    jump: false,
                },
                BallFeel::Responsive,
                0.45,
                false,
                config::FIXED_DT,
            );
            assert_ne!(signal, PhysicsSignal::Fell);
            distance += Vec2::new(ball.position.x, ball.position.z).distance(Vec2::new(
                ball.previous_position.x,
                ball.previous_position.z,
            ));
        }

        assert!(
            distance > 800.0,
            "scripted regression did not reach the reported distance: {distance:.1} m"
        );
    }

    #[test]
    fn neutral_input_does_not_apply_a_hidden_brake() {
        let field = TerrainField::new(0x5EED, config::TERRAIN_INTENSITY_DEFAULT);
        let mut ball = SphereBody::new(field);
        ball.velocity = Vec3::new(0.0, 0.0, 10.0);

        for _ in 0..120 {
            let signal = ball.fixed_step(
                field,
                ControlIntent::default(),
                BallFeel::Responsive,
                0.0,
                false,
                config::FIXED_DT,
            );
            assert_ne!(signal, PhysicsSignal::Fell);
        }

        assert!(
            ball.speed() > 6.5,
            "neutral controls behaved like a brake: {:.2} m/s remained",
            ball.speed()
        );
    }

    #[test]
    fn responsive_input_accelerates_promptly_without_a_brake_path() {
        let field = TerrainField::new(0x5EED, config::TERRAIN_INTENSITY_DEFAULT);
        let mut ball = SphereBody::new(field);
        ball.velocity = Vec3::ZERO;
        let drive = ControlIntent {
            direction: Vec3::Z,
            strength: 1.0,
            jump: false,
        };

        for _ in 0..120 {
            ball.fixed_step(
                field,
                drive,
                BallFeel::Responsive,
                0.0,
                false,
                config::FIXED_DT,
            );
        }

        assert!(
            ball.speed() > 12.0,
            "responsive preset accelerated too slowly: {:.2} m/s after one second",
            ball.speed()
        );
    }

    #[test]
    fn jump_hops_once_from_the_ground_and_not_again_in_midair() {
        let field = TerrainField::new(0x000A_11CE, config::TERRAIN_INTENSITY_DEFAULT);
        let mut ball = SphereBody::new(field);
        let start_y = ball.position.y;
        let jump = ControlIntent {
            jump: true,
            ..ControlIntent::default()
        };

        ball.fixed_step(
            field,
            jump,
            BallFeel::Responsive,
            0.0,
            false,
            config::FIXED_DT,
        );
        let first_jump_velocity = ball.velocity.y;
        assert!(!ball.grounded);
        assert!(first_jump_velocity > 7.0);

        ball.fixed_step(
            field,
            jump,
            BallFeel::Responsive,
            0.0,
            false,
            config::FIXED_DT,
        );
        assert!(ball.velocity.y < first_jump_velocity);
        assert!(ball.position.y > start_y);
    }

    fn assert_surface_clearance(field: TerrainField, x: f32, z: f32) {
        let (floor, normal, contact) = sphere_surface_contact(field, x, z);
        let center = Vec3::new(x, floor, z);
        let surface = Vec3::new(contact.x, field.height(contact.x, contact.y), contact.y);
        let normal_clearance = (center - surface).dot(normal);
        assert!(normal_clearance >= config::BALL_RADIUS + config::BALL_SURFACE_CLEARANCE - 0.002);
        assert!(
            floor - config::BALL_RADIUS
                >= field.height(x, z) + config::BALL_SURFACE_CLEARANCE - 0.002
        );
    }
}
