use crate::{
    config,
    persistence::{BallFeel, GraphicsQuality, TrailStyle, VisualStyle},
    physics::{ControlIntent, PhysicsSignal, SphereBody},
    terrain::{ChunkKey, ChunkMesh, TerrainField, TerrainStreamer},
};
use glam::{Quat, Vec2, Vec3};
use std::{
    collections::{HashMap, HashSet},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GameScreen {
    Splash,
    Menu,
    Tutorial,
    Settings,
    PauseSettings,
    ThemeRestartWarning,
    Achievements,
    Playing,
    Paused,
    GameOver,
}

#[derive(Clone, Copy, Debug)]
pub enum AudioEvent {
    Collect { streak: u32 },
    Party,
    Jump,
    Recovery,
    GameOver,
}

#[derive(Clone, Debug)]
pub struct Collectible {
    pub chunk: ChunkKey,
    pub position: Vec3,
    pub phase: f32,
    pub is_party: bool,
}

#[derive(Clone, Debug)]
pub struct Particle {
    pub position: Vec3,
    pub velocity: Vec3,
    pub age: f32,
    pub lifetime: f32,
    pub color: [f32; 3],
}

#[derive(Clone, Debug)]
pub struct TrailPoint {
    pub position: Vec3,
    pub age: f32,
    pub lifetime: f32,
    pub hue: f32,
}

/// A run-long cosmetic mark anchored to the analytic heightfield. Marks are stored by terrain
/// chunk so only the small visible neighbourhood is submitted to the GPU, while revisiting an
/// earlier part of the run restores the complete line.
#[derive(Clone, Debug)]
pub struct SurfaceTrailPoint {
    pub position: Vec3,
    pub normal: Vec3,
    pub direction: Vec2,
    pub hue: f32,
    pub sequence: u64,
    pub style: TrailStyle,
    pub deformation: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ProgressDelta {
    pub pickups: u64,
    pub distance: f64,
    pub completed_runs: u64,
    pub party_pickups: u64,
    pub best_streak: u32,
}

#[derive(Clone, Copy, Debug)]
pub struct Camera {
    pub position: Vec3,
    pub previous_position: Vec3,
    pub target: Vec3,
    pub previous_target: Vec3,
    heading: Vec3,
}

impl Camera {
    fn new(ball: Vec3) -> Self {
        let heading = Vec3::Z;
        let position = ball - heading * config::CAMERA_DISTANCE + Vec3::Y * config::CAMERA_HEIGHT;
        let target = ball + heading * config::CAMERA_LOOK_AHEAD;
        Self {
            position,
            previous_position: position,
            target,
            previous_target: target,
            heading,
        }
    }

    fn fixed_update(&mut self, ball: &SphereBody, field: TerrainField, sensitivity: f32, dt: f32) {
        self.previous_position = self.position;
        self.previous_target = self.target;
        let desired_position = ball.position - self.heading * config::CAMERA_DISTANCE
            + Vec3::Y * config::CAMERA_HEIGHT;
        let desired_target =
            ball.position + self.heading * config::CAMERA_LOOK_AHEAD + Vec3::Y * 0.1;
        let position_smoothing = 1.0 - (-dt * 4.2 * sensitivity.sqrt()).exp();
        let target_smoothing = 1.0 - (-dt * 7.2).exp();
        self.position = self.position.lerp(desired_position, position_smoothing);
        self.target = self.target.lerp(desired_target, target_smoothing);
        let terrain_clearance = field.height(self.position.x, self.position.z) + 1.45;
        self.position.y = self.position.y.max(terrain_clearance);
    }

    pub fn movement_basis(&self) -> (Vec3, Vec3) {
        let forward = Vec3::new(
            self.target.x - self.position.x,
            0.0,
            self.target.z - self.position.z,
        )
        .normalize_or_zero();
        let forward = if forward.length_squared() < 0.5 {
            Vec3::Z
        } else {
            forward
        };
        let right = Vec3::new(forward.z, 0.0, -forward.x);
        (forward, right)
    }

    pub fn interpolated(&self, alpha: f32) -> (Vec3, Vec3) {
        (
            self.previous_position.lerp(self.position, alpha),
            self.previous_target.lerp(self.target, alpha),
        )
    }
}

pub struct Game {
    pub screen: GameScreen,
    pub ball: SphereBody,
    pub camera: Camera,
    pub score: u64,
    pub streak: u32,
    pub distance: f32,
    pub elapsed: f32,
    pub seed: u64,
    pub collectibles: HashMap<u64, Collectible>,
    pub particles: Vec<Particle>,
    pub trail: Vec<TrailPoint>,
    pub surface_trail: HashMap<ChunkKey, Vec<SurfaceTrailPoint>>,
    pub trail_deformation: bool,
    pub recovery_notice: f32,
    pub party_time: f32,
    pub visual_style: VisualStyle,
    pub terrain_intensity: f32,
    splash_time: f32,
    streak_time: f32,
    trail_emit_timer: f32,
    surface_trail_style: TrailStyle,
    surface_trail_cursor: Option<Vec2>,
    surface_trail_sequence: u64,
    streamer: TerrainStreamer,
    collected: HashSet<u64>,
    terrain_incoming: Vec<ChunkMesh>,
    terrain_outgoing: Vec<ChunkKey>,
    audio_events: Vec<AudioEvent>,
    progress_delta: ProgressDelta,
}

impl Game {
    #[cfg(test)]
    pub fn new(terrain_intensity: f32) -> Self {
        Self::with_style(terrain_intensity, VisualStyle::Classic)
    }

    pub fn with_style(terrain_intensity: f32, visual_style: VisualStyle) -> Self {
        let seed = fresh_seed();
        let streamer = TerrainStreamer::new(seed, terrain_intensity, visual_style);
        let field = streamer.field();
        let ball = SphereBody::new(field);
        let camera = Camera::new(ball.position);
        Self {
            screen: GameScreen::Splash,
            ball,
            camera,
            score: 0,
            streak: 0,
            distance: 0.0,
            elapsed: 0.0,
            seed,
            collectibles: HashMap::new(),
            particles: Vec::new(),
            trail: Vec::new(),
            surface_trail: HashMap::new(),
            trail_deformation: true,
            recovery_notice: 0.0,
            party_time: 0.0,
            visual_style,
            terrain_intensity,
            splash_time: 0.0,
            streak_time: 0.0,
            trail_emit_timer: 0.0,
            surface_trail_style: TrailStyle::Smoke,
            surface_trail_cursor: None,
            surface_trail_sequence: 0,
            streamer,
            collected: HashSet::new(),
            terrain_incoming: Vec::new(),
            terrain_outgoing: Vec::new(),
            audio_events: Vec::new(),
            progress_delta: ProgressDelta::default(),
        }
    }

    pub fn tick_splash(&mut self, dt: f32) {
        if self.screen == GameScreen::Splash {
            self.splash_time += dt;
            if self.splash_time >= 1.65 {
                self.screen = GameScreen::Menu;
            }
        }
    }

    pub fn start_same_seed(&mut self, terrain_intensity: f32) {
        self.start_run(self.seed, terrain_intensity);
    }

    pub fn start_new_seed(&mut self, terrain_intensity: f32) {
        self.start_run(fresh_seed() ^ self.seed.rotate_left(17), terrain_intensity);
    }

    /// Deterministic developer launch path used to reproduce a player-reported seed exactly.
    pub fn start_seed_for_preview(&mut self, seed: u64, terrain_intensity: f32) {
        self.start_run(seed, terrain_intensity);
    }

    fn start_run(&mut self, seed: u64, terrain_intensity: f32) {
        self.seed = seed;
        self.terrain_intensity = terrain_intensity;
        self.streamer
            .reset(seed, terrain_intensity, self.visual_style);
        let field = self.streamer.field();
        self.ball = SphereBody::new(field);
        self.camera = Camera::new(self.ball.position);
        self.score = 0;
        self.streak = 0;
        self.streak_time = 0.0;
        self.distance = 0.0;
        self.elapsed = 0.0;
        self.collectibles.clear();
        self.collected.clear();
        self.particles.clear();
        self.trail.clear();
        self.surface_trail.clear();
        self.recovery_notice = 0.0;
        self.party_time = 0.0;
        self.trail_emit_timer = 0.0;
        self.surface_trail_cursor = None;
        self.surface_trail_sequence = 0;
        self.screen = GameScreen::Playing;
    }

    /// Rebuilds the quiet menu backdrop immediately after the setting is applied, so the chosen
    /// relief is also reflected before the next run starts.
    pub fn apply_terrain_settings(&mut self, terrain_intensity: f32, visual_style: VisualStyle) {
        self.visual_style = visual_style;
        self.terrain_intensity = terrain_intensity;
        self.streamer
            .reset(self.seed, terrain_intensity, visual_style);
        let field = self.streamer.field();
        self.ball = SphereBody::new(field);
        self.camera = Camera::new(self.ball.position);
        self.collectibles.clear();
        self.collected.clear();
        self.particles.clear();
        self.trail.clear();
        self.surface_trail.clear();
        self.party_time = 0.0;
        self.trail_emit_timer = 0.0;
        self.surface_trail_cursor = None;
        self.surface_trail_sequence = 0;
    }

    /// Rebuilds streamed geometry around the current run without changing its score, timer,
    /// distance, collected-item history, party state, or progression. Persistent surface marks
    /// are reprojected onto the adjusted heightfield so changing relief while paused is seamless.
    pub fn apply_live_terrain_intensity(&mut self, terrain_intensity: f32) {
        self.terrain_intensity = terrain_intensity;
        self.streamer
            .reset(self.seed, terrain_intensity, self.visual_style);
        let field = self.streamer.field();
        self.ball.reproject_to_surface(field);
        self.camera.position.y = self
            .camera
            .position
            .y
            .max(field.height(self.camera.position.x, self.camera.position.z) + 1.45);
        self.camera.previous_position = self.camera.position;
        self.camera.target =
            self.ball.position + self.camera.heading * config::CAMERA_LOOK_AHEAD + Vec3::Y * 0.1;
        self.camera.previous_target = self.camera.target;
        self.collectibles.clear();
        for points in self.surface_trail.values_mut() {
            for point in points {
                point.position.y = field.height(point.position.x, point.position.z);
                point.normal = field.normal(point.position.x, point.position.z);
            }
        }
        self.surface_trail_cursor = None;
    }

    /// A visual theme changes terrain topology and gameplay tuning, so it deliberately begins
    /// the same seed again after the player confirms the restart warning.
    pub fn restart_same_seed_with_settings(
        &mut self,
        terrain_intensity: f32,
        visual_style: VisualStyle,
    ) {
        self.visual_style = visual_style;
        self.start_same_seed(terrain_intensity);
    }

    pub fn apply_trail_settings(&mut self, style: TrailStyle, deformation: bool) {
        self.surface_trail_style = style;
        self.trail_deformation = deformation;
        self.surface_trail_cursor = None;
    }

    pub fn toggle_pause(&mut self) {
        self.screen = match self.screen {
            GameScreen::Playing => GameScreen::Paused,
            GameScreen::Paused => GameScreen::Playing,
            other => other,
        };
    }

    pub fn update_streaming(&mut self, quality: GraphicsQuality) {
        self.streamer.update(self.ball.position, quality);
        let (incoming, outgoing) = self.streamer.take_changes();
        if !outgoing.is_empty() {
            let removed: HashSet<_> = outgoing.iter().copied().collect();
            self.collectibles
                .retain(|_, item| !removed.contains(&item.chunk));
        }
        for chunk in &incoming {
            for spawn in &chunk.collectibles {
                if !self.collected.contains(&spawn.id) {
                    self.collectibles.insert(
                        spawn.id,
                        Collectible {
                            chunk: chunk.key,
                            position: spawn.position,
                            phase: ((spawn.id >> 32) as f32 / u32::MAX as f32)
                                * std::f32::consts::TAU,
                            is_party: spawn.is_party,
                        },
                    );
                }
            }
        }
        self.terrain_incoming.extend(incoming);
        self.terrain_outgoing.extend(outgoing);
    }

    pub fn take_terrain_changes(&mut self) -> (Vec<ChunkMesh>, Vec<ChunkKey>) {
        (
            std::mem::take(&mut self.terrain_incoming),
            std::mem::take(&mut self.terrain_outgoing),
        )
    }

    pub fn fixed_step(
        &mut self,
        movement: Vec2,
        jump: bool,
        camera_sensitivity: f32,
        ball_feel: BallFeel,
        dt: f32,
    ) {
        if self.screen != GameScreen::Playing {
            return;
        }
        self.elapsed += dt;
        self.recovery_notice = (self.recovery_notice - dt).max(0.0);
        self.party_time = (self.party_time - dt).max(0.0);
        self.streak_time = (self.streak_time - dt).max(0.0);
        if self.streak_time <= 0.0 {
            self.streak = 0;
        }

        let (forward, right) = self.camera.movement_basis();
        let world_direction = (right * movement.x + forward * movement.y).normalize_or_zero();
        let difficulty = (self.distance / 1800.0 + self.score as f32 / 45_000.0).clamp(0.0, 1.0);
        let hazards_rendered = self.streamer.is_rendered_at(self.ball.position);
        let jumped = jump && self.ball.grounded;
        let signal = self.ball.fixed_step(
            self.streamer.field(),
            ControlIntent {
                direction: world_direction,
                strength: movement.length().min(1.0),
                jump,
            },
            ball_feel,
            difficulty,
            hazards_rendered,
            dt,
        );
        if jumped {
            self.audio_events.push(AudioEvent::Jump);
        }
        match signal {
            PhysicsSignal::Recovered => {
                self.recovery_notice = 2.4;
                self.audio_events.push(AudioEvent::Recovery);
            }
            PhysicsSignal::Fell => self.finish_run(),
            PhysicsSignal::None => {}
        }

        let delta = Vec2::new(
            self.ball.position.x - self.ball.previous_position.x,
            self.ball.position.z - self.ball.previous_position.z,
        )
        .length();
        self.distance += delta;
        self.progress_delta.distance += delta as f64;
        self.camera
            .fixed_update(&self.ball, self.streamer.field(), camera_sensitivity, dt);
        self.collect_nearby();
        self.update_particles(dt);
        self.update_surface_trail();
        self.update_trail(dt);
    }

    fn collect_nearby(&mut self) {
        let mut hits = Vec::new();
        for (&id, collectible) in &self.collectibles {
            if self.ball.position.distance(collectible.position) <= config::COLLECT_RADIUS {
                hits.push(id);
            }
        }
        for id in hits {
            if let Some(item) = self.collectibles.remove(&id) {
                self.collected.insert(id);
                self.streak += 1;
                self.streak_time = config::STREAK_WINDOW;
                if item.is_party {
                    self.party_time = 30.0;
                    self.progress_delta.party_pickups += 1;
                    self.audio_events.push(AudioEvent::Party);
                }
                let streak_multiplier = 1 + (self.streak.saturating_sub(1) / 5) as u64;
                let party_multiplier = if self.party_time > 0.0 { 4 } else { 1 };
                self.score += config::BASE_COLLECTIBLE_SCORE * streak_multiplier * party_multiplier;
                self.ball.pulse = 1.0;
                self.spawn_burst(item.position, id, item.is_party || self.party_time > 0.0);
                if !item.is_party {
                    self.audio_events.push(AudioEvent::Collect {
                        streak: self.streak,
                    });
                }
                self.progress_delta.pickups += 1;
                self.progress_delta.best_streak = self.progress_delta.best_streak.max(self.streak);
            }
        }
    }

    fn spawn_burst(&mut self, position: Vec3, seed: u64, rainbow: bool) {
        for i in 0..18u64 {
            let h = mix64(seed ^ i.wrapping_mul(0x9e3779b97f4a7c15));
            let angle = unit(h) * std::f32::consts::TAU;
            let lift = 1.4 + unit(h.rotate_left(19)) * 3.8;
            let radial = 1.4 + unit(h.rotate_left(41)) * 3.2;
            self.particles.push(Particle {
                position,
                velocity: Vec3::new(angle.cos() * radial, lift, angle.sin() * radial),
                age: 0.0,
                lifetime: 0.48 + unit(h.rotate_left(8)) * 0.42,
                color: if rainbow {
                    hue_to_rgb((unit(h.rotate_left(27)) + i as f32 / 18.0) % 1.0)
                } else {
                    [1.0, 0.49, 0.05]
                },
            });
        }
    }

    fn update_particles(&mut self, dt: f32) {
        for particle in &mut self.particles {
            particle.age += dt;
            particle.velocity.y -= 8.0 * dt;
            particle.position += particle.velocity * dt;
        }
        self.particles
            .retain(|particle| particle.age < particle.lifetime);
    }

    fn update_trail(&mut self, dt: f32) {
        for point in &mut self.trail {
            point.age += dt;
        }
        self.trail.retain(|point| point.age < point.lifetime);

        if !self.party_active() {
            self.trail_emit_timer = 0.0;
            return;
        }

        const EMIT_INTERVAL: f32 = 0.045;
        self.trail_emit_timer += dt;
        while self.trail_emit_timer >= EMIT_INTERVAL {
            self.trail_emit_timer -= EMIT_INTERVAL;
            self.trail.push(TrailPoint {
                position: self.ball.position + Vec3::Y * 0.03,
                age: 0.0,
                lifetime: 1.28,
                hue: (self.elapsed * 0.42 + self.trail.len() as f32 * 0.071).fract(),
            });
        }
        if self.trail.len() > 72 {
            self.trail.drain(..self.trail.len() - 72);
        }
    }

    fn update_surface_trail(&mut self) {
        if self.surface_trail_style == TrailStyle::Off || !self.ball.grounded {
            self.surface_trail_cursor = None;
            return;
        }

        let field = self.streamer.field();
        let current = Vec2::new(self.ball.position.x, self.ball.position.z);
        let Some(mut cursor) = self.surface_trail_cursor else {
            self.push_surface_mark(field, current, Vec2::Y);
            self.surface_trail_cursor = Some(current);
            return;
        };

        let offset = current - cursor;
        let distance = offset.length();
        if distance < config::SURFACE_TRAIL_SPACING {
            return;
        }
        let direction = offset / distance;
        let steps = (distance / config::SURFACE_TRAIL_SPACING).floor() as usize;
        for _ in 0..steps.min(12) {
            cursor += direction * config::SURFACE_TRAIL_SPACING;
            self.push_surface_mark(field, cursor, direction);
        }
        self.surface_trail_cursor = Some(cursor);
    }

    fn push_surface_mark(&mut self, field: TerrainField, point: Vec2, direction: Vec2) {
        let key = ChunkKey::from_world(point.x, point.y);
        let sequence = self.surface_trail_sequence;
        let hue = (sequence as f32 * 0.031_25 + 0.57).fract();
        self.surface_trail_sequence = self.surface_trail_sequence.wrapping_add(1);
        self.surface_trail
            .entry(key)
            .or_default()
            .push(SurfaceTrailPoint {
                position: Vec3::new(point.x, field.height(point.x, point.y), point.y),
                normal: field.normal(point.x, point.y),
                direction,
                hue,
                sequence,
                style: self.surface_trail_style,
                deformation: self.trail_deformation,
            });
    }

    pub fn interpolated_ball(&self, alpha: f32) -> (Vec3, Quat) {
        (
            self.ball.previous_position.lerp(self.ball.position, alpha),
            self.ball.previous_rotation.slerp(self.ball.rotation, alpha),
        )
    }

    pub fn take_audio_events(&mut self) -> Vec<AudioEvent> {
        std::mem::take(&mut self.audio_events)
    }

    pub fn take_progress_delta(&mut self) -> ProgressDelta {
        std::mem::take(&mut self.progress_delta)
    }

    pub fn party_active(&self) -> bool {
        self.party_time > 0.0
    }

    /// Developer-facing visual QA mode used by the documented `--party-preview` launch flag.
    pub fn enable_party_preview(&mut self) {
        self.party_time = 30.0;
        let x = self.ball.position.x + 4.5;
        let z = self.ball.position.z + 5.0;
        let position = Vec3::new(x, self.streamer.field().height(x, z) + 1.28, z);
        self.collectibles.insert(
            u64::MAX,
            Collectible {
                chunk: ChunkKey { x: 0, z: 0 },
                position,
                phase: 0.0,
                is_party: true,
            },
        );
    }

    /// Places the camera just before the nearest generated tear for deterministic visual QA.
    /// The associated command-line preview freezes simulation in `main`, so it cannot consume a
    /// real run or progression.
    pub fn enable_tear_preview(&mut self) {
        let field = self.streamer.field();
        let tear = (-320..=320)
            .step_by(2)
            .flat_map(|z| {
                (-320..=320)
                    .step_by(2)
                    .map(move |x| Vec2::new(x as f32, z as f32))
            })
            .filter(|point| field.is_tear(point.x, point.y))
            .min_by(|a, b| {
                a.length_squared()
                    .partial_cmp(&b.length_squared())
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
        let Some(tear) = tear else { return };
        let mut preview = tear - Vec2::Y * 14.0;
        while field.is_tear(preview.x, preview.y) {
            preview -= Vec2::Y * 2.0;
        }
        self.ball.position.x = preview.x;
        self.ball.position.z = preview.y;
        self.ball.velocity = Vec3::ZERO;
        self.ball.reproject_to_surface(field);
        self.camera = Camera::new(self.ball.position);
        self.collectibles.clear();
        self.screen = GameScreen::Playing;
    }

    fn finish_run(&mut self) {
        if self.screen == GameScreen::Playing {
            self.screen = GameScreen::GameOver;
            self.audio_events.push(AudioEvent::GameOver);
            self.progress_delta.completed_runs += 1;
        }
    }
}

fn fresh_seed() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos() as u64)
        .unwrap_or(0x51A7_EE11)
        ^ 0xA5A5_31C3_9E37_79B9
}

fn mix64(mut x: u64) -> u64 {
    x ^= x >> 30;
    x = x.wrapping_mul(0xbf58476d1ce4e5b9);
    x ^= x >> 27;
    x = x.wrapping_mul(0x94d049bb133111eb);
    x ^ (x >> 31)
}

fn unit(value: u64) -> f32 {
    ((value >> 40) as f32) / ((1u32 << 24) as f32)
}

pub fn hue_to_rgb(hue: f32) -> [f32; 3] {
    let h = (hue.fract() * 6.0).max(0.0);
    let x = 1.0 - (h % 2.0 - 1.0).abs();
    match h as u32 {
        0 => [1.0, x, 0.08],
        1 => [x, 1.0, 0.08],
        2 => [0.08, 1.0, x],
        3 => [0.08, x, 1.0],
        4 => [x, 0.08, 1.0],
        _ => [1.0, 0.08, x],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn party_pickup_activates_timer_and_quadruples_points() {
        let mut game = Game::new(config::TERRAIN_INTENSITY_DEFAULT);
        game.screen = GameScreen::Playing;
        game.collectibles.insert(
            1,
            Collectible {
                chunk: ChunkKey { x: 0, z: 0 },
                position: game.ball.position,
                phase: 0.0,
                is_party: true,
            },
        );
        game.collect_nearby();
        assert_eq!(game.party_time, 30.0);
        assert_eq!(game.score, config::BASE_COLLECTIBLE_SCORE * 4);

        game.collectibles.insert(
            2,
            Collectible {
                chunk: ChunkKey { x: 0, z: 0 },
                position: game.ball.position,
                phase: 0.0,
                is_party: false,
            },
        );
        game.collect_nearby();
        assert_eq!(game.score, config::BASE_COLLECTIBLE_SCORE * 8);
        let progress = game.take_progress_delta();
        assert_eq!(progress.pickups, 2);
        assert_eq!(progress.party_pickups, 1);
        assert_eq!(progress.best_streak, 2);
    }

    #[test]
    fn rainbow_color_cycle_wraps_cleanly() {
        assert_eq!(hue_to_rgb(0.0), hue_to_rgb(1.0));
        assert_ne!(hue_to_rgb(0.0), hue_to_rgb(0.5));
    }

    #[test]
    fn party_trail_emits_and_fades() {
        let mut game = Game::new(config::TERRAIN_INTENSITY_DEFAULT);
        game.party_time = 1.0;
        game.update_trail(0.1);
        assert_eq!(game.trail.len(), 2);
        game.party_time = 0.0;
        game.update_trail(2.0);
        assert!(game.trail.is_empty());
    }

    #[test]
    fn surface_trail_persists_without_party_mode() {
        let mut game = Game::new(config::TERRAIN_INTENSITY_DEFAULT);
        game.apply_trail_settings(TrailStyle::Graphite, true);
        game.ball.grounded = true;
        game.update_surface_trail();
        game.ball.position.x += config::SURFACE_TRAIL_SPACING * 2.2;
        game.update_surface_trail();
        let count: usize = game.surface_trail.values().map(Vec::len).sum();
        assert!(count >= 3);
        assert_eq!(game.party_time, 0.0);
        assert!(game
            .surface_trail
            .values()
            .flatten()
            .all(|point| point.deformation));
    }

    #[test]
    fn disabled_surface_trail_emits_no_marks() {
        let mut game = Game::new(config::TERRAIN_INTENSITY_DEFAULT);
        game.apply_trail_settings(TrailStyle::Off, true);
        game.ball.grounded = true;
        game.update_surface_trail();
        assert!(game.surface_trail.is_empty());
    }

    #[test]
    fn live_relief_changes_preserve_the_active_run() {
        let mut game = Game::new(config::TERRAIN_INTENSITY_DEFAULT);
        game.screen = GameScreen::Paused;
        game.score = 4_200;
        game.distance = 731.5;
        game.elapsed = 42.0;
        game.party_time = 9.0;
        let seed = game.seed;
        let horizontal_position = Vec2::new(game.ball.position.x, game.ball.position.z);

        game.apply_live_terrain_intensity(1.35);

        assert_eq!(game.screen, GameScreen::Paused);
        assert_eq!(game.seed, seed);
        assert_eq!(game.score, 4_200);
        assert_eq!(game.distance, 731.5);
        assert_eq!(game.elapsed, 42.0);
        assert_eq!(game.party_time, 9.0);
        assert_eq!(
            Vec2::new(game.ball.position.x, game.ball.position.z),
            horizontal_position
        );
        assert!((game.terrain_intensity - 1.35).abs() < f32::EPSILON);
    }
}
