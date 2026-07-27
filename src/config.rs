//! Central tuning values. Keeping the feel in one place makes iteration predictable.

pub const WINDOW_TITLE: &str = "RIDGELINE";
pub const INITIAL_WIDTH: u32 = 2560;
pub const INITIAL_HEIGHT: u32 = 1080;

pub const FIXED_DT: f32 = 1.0 / 120.0;
pub const MAX_FRAME_TIME: f32 = 0.1;

pub const BALL_RADIUS: f32 = 0.92;
pub const GRAVITY: f32 = 24.0;
pub const GROUND_ACCELERATION: f32 = 28.0;
pub const AIR_ACCELERATION: f32 = 5.0;
pub const MAX_SPEED_BASE: f32 = 28.0;
pub const MAX_SPEED_CAP: f32 = 45.0;
pub const ROLLING_RESISTANCE: f32 = 0.32;
pub const JUMP_SPEED: f32 = 8.4;
pub const TEAR_WARNING_ARM_TIME: f32 = 0.06;
pub const GROUND_SNAP: f32 = 0.20;
/// Small visual safety margin above the mathematical surface. This also absorbs the difference
/// between the analytic heightfield and its piecewise-linear render mesh.
pub const BALL_SURFACE_CLEARANCE: f32 = 0.075;

pub const CHUNK_SIZE: f32 = 48.0;
pub const CHUNK_RESOLUTION_LOW: u32 = 28;
pub const CHUNK_RESOLUTION_MEDIUM: u32 = 40;
pub const CHUNK_RESOLUTION_HIGH: u32 = 52;

pub const TERRAIN_BASE_FREQUENCY: f32 = 0.0135;
pub const TERRAIN_BASE_AMPLITUDE: f32 = 23.5;
pub const TERRAIN_INTENSITY_DEFAULT: f32 = 2.15;
pub const START_SAFE_RADIUS: f32 = 92.0;

/// High-oblique orthographic framing measured against the supplied 1600 × 900 reference. The
/// negative look offset places the ball in the upper third and exposes the terrain rolling away
/// beneath it, matching the reference composition instead of reading like a chase camera.
pub const CAMERA_DISTANCE: f32 = 28.0;
pub const CAMERA_HEIGHT: f32 = 47.0;
pub const CAMERA_LOOK_AHEAD: f32 = -3.0;
pub const CAMERA_VIEW_HEIGHT: f32 = 40.0;

pub const SURFACE_TRAIL_SPACING: f32 = 0.54;
pub const TRAIL_DEFORMATION_MARKS: usize = 64;

pub const COLLECT_RADIUS: f32 = 1.45;
pub const STREAK_WINDOW: f32 = 3.4;
pub const BASE_COLLECTIBLE_SCORE: u64 = 100;
