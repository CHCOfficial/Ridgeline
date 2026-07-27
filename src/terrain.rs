//! Deterministic, seamless terrain and background chunk generation.
//!
//! Heights are sampled in world space, so neighbouring chunks share bit-identical border
//! positions. The physics controller samples this same field directly; collision can therefore
//! never lag behind or disagree with the mesh streamer.

use crate::{
    config,
    persistence::{GraphicsQuality, VisualStyle},
};
use bytemuck::{Pod, Zeroable};
use glam::{Vec2, Vec3};
use std::{
    collections::HashSet,
    sync::mpsc::{self, Receiver, Sender},
    thread,
};

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub struct ChunkKey {
    pub x: i32,
    pub z: i32,
}

impl ChunkKey {
    pub fn from_world(x: f32, z: f32) -> Self {
        Self {
            x: (x / config::CHUNK_SIZE).floor() as i32,
            z: (z / config::CHUNK_SIZE).floor() as i32,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct TerrainVertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    /// x = dark rim strength, y = recessed chasm surface/wall.
    pub tear_info: [f32; 2],
}

#[derive(Clone, Debug)]
pub struct CollectibleSpawn {
    pub id: u64,
    pub position: Vec3,
    pub is_party: bool,
}

#[derive(Debug)]
pub struct ChunkMesh {
    pub key: ChunkKey,
    pub generation: u64,
    pub vertices: Vec<TerrainVertex>,
    pub indices: Vec<u32>,
    pub collectibles: Vec<CollectibleSpawn>,
}

#[derive(Clone, Copy, Debug)]
pub struct TerrainField {
    seed: u64,
    peak_intensity: f32,
    visual_style: VisualStyle,
}

impl TerrainField {
    /// Collision is deliberately inset well inside the rendered opening. A second, mesh-aware
    /// gate in `is_tear` guarantees that the shader actually cuts the surface at the same point
    /// on every graphics-quality tessellation before physics can treat it as lethal.
    const TEAR_LETHAL_METRIC: f32 = 0.34;
    const TEAR_RENDERED_LETHAL_METRIC: f32 = 0.82;
    const TEAR_OPENING_METRIC: f32 = 1.24;
    const TEAR_MAX_HALF_LENGTH: f32 = 5.2;
    const TEAR_SAFE_RADIUS: f32 = config::START_SAFE_RADIUS + 36.0;

    #[cfg(test)]
    pub fn new(seed: u64, peak_intensity: f32) -> Self {
        Self::with_style(seed, peak_intensity, VisualStyle::Classic)
    }

    pub fn with_style(seed: u64, peak_intensity: f32, visual_style: VisualStyle) -> Self {
        Self {
            seed,
            peak_intensity: peak_intensity.clamp(0.60, 2.60),
            visual_style,
        }
    }

    pub fn seed(&self) -> u64 {
        self.seed
    }

    /// Layered value noise plus a low-frequency domain warp. Distance is encoded entirely from
    /// world position, preserving deterministic runs while allowing formations to intensify.
    pub fn height(&self, x: f32, z: f32) -> f32 {
        let p = Vec2::new(x, z);
        let distance = p.length();
        let safe = smoothstep(
            config::START_SAFE_RADIUS * 0.35,
            config::START_SAFE_RADIUS,
            distance,
        );
        let intensity = 0.54 + 0.46 * smoothstep(110.0, 1350.0, distance);

        let (warp_scale, warp_strength, macro_scale, macro_power) = match self.visual_style {
            VisualStyle::Classic => (config::TERRAIN_BASE_FREQUENCY * 0.58, 1.0, 0.0074, 0.72),
            VisualStyle::Vaporwave => (0.0050, 1.38, 0.0048, 0.64),
            VisualStyle::Dark => (0.0068, 1.72, 0.0061, 0.53),
        };
        let warp_x = self.fbm(p * warp_scale + Vec2::new(19.3, -8.1), 3);
        let warp_z = self.fbm(p * warp_scale + Vec2::new(-31.7, 13.9), 3);
        let warped = p + Vec2::new(warp_x, warp_z) * (13.0 + 17.0 * intensity) * warp_strength;

        // A deliberately broad, sign-preserving landform layer creates the distinct bowls and
        // shoulders visible in the reference. The sub-linear shaping expands middle values away
        // from zero without introducing cliffs or discontinuities.
        let macro_noise = self.fbm(warped * macro_scale + Vec2::new(-16.0, 38.0), 4);
        let macro_form = macro_noise.signum() * macro_noise.abs().powf(macro_power);
        let (broad_scale, rolling_scale, ridge_scale, ridge_power) = match self.visual_style {
            VisualStyle::Classic => (config::TERRAIN_BASE_FREQUENCY, 0.025, 0.0082, 2.15),
            VisualStyle::Vaporwave => (0.0090, 0.017, 0.0062, 2.45),
            VisualStyle::Dark => (0.0115, 0.022, 0.0074, 1.72),
        };
        let broad = self.fbm(warped * broad_scale, 5);
        let rolling = self.fbm(warped * rolling_scale + Vec2::new(7.0, 23.0), 4);
        let ridge_noise = self.fbm(warped * ridge_scale + Vec2::new(71.0, -29.0), 4);
        let ridges = (1.0 - ridge_noise.abs()).powf(ridge_power) - 0.46;

        // The Dark profile adds a wide, warped signed bank layer. It bends the terrain into the
        // high-walled channels from the charcoal icon while staying continuous for collision.
        let bank_noise = self.fbm(warped * 0.0049 + Vec2::new(-84.0, 57.0), 4);
        let banks = bank_noise.signum() * bank_noise.abs().powf(0.46);

        // Broad low-noise regions gently flatten the landscape into occasional recovery areas.
        let recovery_scale = match self.visual_style {
            VisualStyle::Classic => 0.0037,
            VisualStyle::Vaporwave => 0.0029,
            VisualStyle::Dark => 0.0033,
        };
        let recovery_signal = self.value_noise(p * recovery_scale + Vec2::splat(44.0));
        let recovery = smoothstep(0.52, 0.84, recovery_signal);
        let (formed, style_gain) = match self.visual_style {
            VisualStyle::Classic => (
                macro_form * 0.70 + broad * 0.34 + rolling * 0.16 + ridges * 0.22,
                1.0,
            ),
            VisualStyle::Vaporwave => (
                macro_form * 0.88 + broad * 0.38 + rolling * 0.10 + ridges * 0.17,
                1.16,
            ),
            VisualStyle::Dark => (
                macro_form * 0.76 + broad * 0.25 + rolling * 0.18 + ridges * 0.31 + banks * 0.40,
                1.20,
            ),
        };
        let amplitude =
            config::TERRAIN_BASE_AMPLITUDE * intensity * style_gain * (1.0 - 0.24 * recovery);
        let formed = formed * amplitude;

        // A shallow, readable launch basin mirrors the reference and guarantees a fair start.
        let start_basin = -0.75 * (1.0 - smoothstep(0.0, 34.0, distance));
        start_basin + formed * self.peak_intensity * (0.16 + 0.84 * safe)
    }

    pub fn normal(&self, x: f32, z: f32) -> Vec3 {
        let epsilon = 0.22;
        let dx = self.height(x + epsilon, z) - self.height(x - epsilon, z);
        let dz = self.height(x, z + epsilon) - self.height(x, z - epsilon);
        Vec3::new(-dx, epsilon * 2.0, -dz).normalize()
    }

    pub fn slope(&self, x: f32, z: f32) -> f32 {
        1.0 - self.normal(x, z).y
    }

    /// Sparse, seed-stable elliptical slashes form real gaps in the rendered/collision surface.
    /// The metric is below one inside a tear and grows outward from its irregular edge.
    fn tear_metric(&self, x: f32, z: f32) -> f32 {
        const CELL: f32 = 124.0;
        const SPAWN_CHANCE: f32 = 0.26;

        let p = Vec2::new(x, z);
        if p.length() <= Self::TEAR_SAFE_RADIUS {
            return f32::INFINITY;
        }
        let grid = (p / CELL).floor().as_ivec2();
        let mut closest = f32::INFINITY;
        for cell_z in (grid.y - 1)..=(grid.y + 1) {
            for cell_x in (grid.x - 1)..=(grid.x + 1) {
                let hash = hash_u64(self.seed ^ 0x5445_4152_5F4D_4150, cell_x, cell_z);
                if unit_float(hash.rotate_left(7)) >= SPAWN_CHANCE {
                    continue;
                }
                let jitter = Vec2::new(
                    unit_float(hash.rotate_left(19)) - 0.5,
                    unit_float(hash.rotate_left(37)) - 0.5,
                ) * CELL
                    * 0.62;
                let center =
                    (Vec2::new(cell_x as f32, cell_z as f32) + Vec2::splat(0.5)) * CELL + jitter;
                // Keep the complete opening and its outer warning glow beyond the protected
                // launch basin, not merely the lethal centre line.
                if center.length() < Self::TEAR_SAFE_RADIUS + Self::TEAR_MAX_HALF_LENGTH * 2.1 + 8.0
                {
                    continue;
                }

                let angle = unit_float(hash.rotate_left(51)) * std::f32::consts::TAU;
                let axis = Vec2::new(angle.cos(), angle.sin());
                let side = Vec2::new(-axis.y, axis.x);
                let delta = p - center;
                let along = delta.dot(axis);
                let half_length = 3.4 + unit_float(hash.rotate_left(29)) * 1.8;
                let half_width = 0.90 + unit_float(hash.rotate_left(43)) * 0.30;
                let bend = (along / half_length * std::f32::consts::PI).sin()
                    * (0.16 + unit_float(hash.rotate_left(11)) * 0.18)
                    + (along / half_length * std::f32::consts::TAU * 1.5).sin() * 0.07;
                let across = delta.dot(side) - bend;
                let metric = ((along / half_length).powi(2) + (across / half_width).powi(2)).sqrt();
                closest = closest.min(metric);
            }
        }
        closest
    }

    pub fn is_tear(&self, x: f32, z: f32) -> bool {
        self.tear_metric(x, z) < Self::TEAR_LETHAL_METRIC
            && [
                config::CHUNK_RESOLUTION_LOW,
                config::CHUNK_RESOLUTION_MEDIUM,
                config::CHUNK_RESOLUTION_HIGH,
            ]
            .into_iter()
            .all(|resolution| {
                self.rendered_tear_metric(x, z, resolution) < Self::TEAR_RENDERED_LETHAL_METRIC
            })
    }

    /// True only where the opening itself is present in every supported terrain tessellation.
    /// Physics uses this broader visible region to arm a tear before its inset core can become
    /// lethal, preventing a one-sample or not-yet-rendered hazard from ending a run.
    pub fn has_visible_tear_warning(&self, x: f32, z: f32) -> bool {
        self.is_visible_tear(x, z)
            && [
                config::CHUNK_RESOLUTION_LOW,
                config::CHUNK_RESOLUTION_MEDIUM,
                config::CHUNK_RESOLUTION_HIGH,
            ]
            .into_iter()
            .all(|resolution| {
                self.rendered_tear_metric(x, z, resolution) < Self::TEAR_OPENING_METRIC
            })
    }

    /// Samples the sphere's projected underside instead of relying on its centre point. Requiring
    /// several samples inside the renderer-confirmed opening gives stable edge detection without
    /// making a single grazing pixel lethal.
    pub fn ball_overlaps_visible_tear(&self, x: f32, z: f32, radius: f32) -> bool {
        if self.tear_metric(x, z) >= 2.35 {
            return false;
        }
        let ring_radius = radius * 0.72;
        let mut hits = usize::from(self.has_visible_tear_warning(x, z));
        for index in 0..8 {
            let angle = index as f32 * std::f32::consts::TAU / 8.0;
            hits += usize::from(self.has_visible_tear_warning(
                x + angle.cos() * ring_radius,
                z + angle.sin() * ring_radius,
            ));
            if hits >= 3 {
                return true;
            }
        }
        false
    }

    fn is_visible_tear(&self, x: f32, z: f32) -> bool {
        self.tear_metric(x, z) < Self::TEAR_OPENING_METRIC
    }

    /// Reproduces the top mesh's two-triangle interpolation in world space. The fragment shader
    /// discards a point only when this interpolated value is below `TEAR_OPENING_METRIC`; using a
    /// stricter threshold for collision leaves a plainly visible warning around the whole lethal
    /// core and prevents sub-vertex analytic tears from becoming invisible traps.
    fn rendered_tear_metric(&self, x: f32, z: f32, resolution: u32) -> f32 {
        let step = config::CHUNK_SIZE / resolution as f32;
        let chunk_x = (x / config::CHUNK_SIZE).floor() * config::CHUNK_SIZE;
        let chunk_z = (z / config::CHUNK_SIZE).floor() * config::CHUNK_SIZE;
        let grid_x = ((x - chunk_x) / step).clamp(0.0, resolution as f32);
        let grid_z = ((z - chunk_z) / step).clamp(0.0, resolution as f32);
        let cell_x = grid_x.floor().min(resolution.saturating_sub(1) as f32);
        let cell_z = grid_z.floor().min(resolution.saturating_sub(1) as f32);
        let u = (grid_x - cell_x).clamp(0.0, 1.0);
        let v = (grid_z - cell_z).clamp(0.0, 1.0);
        let x0 = chunk_x + cell_x * step;
        let z0 = chunk_z + cell_z * step;
        let sample = |sample_x: f32, sample_z: f32| self.tear_metric(sample_x, sample_z).min(3.0);
        let a = sample(x0, z0);
        let b = sample(x0 + step, z0);
        let c = sample(x0, z0 + step);
        let d = sample(x0 + step, z0 + step);

        if u + v <= 1.0 {
            a * (1.0 - u - v) + b * u + c * v
        } else {
            b * (1.0 - v) + c * (1.0 - u) + d * (u + v - 1.0)
        }
    }

    fn fbm(&self, p: Vec2, octaves: u32) -> f32 {
        let mut value = 0.0;
        let mut amplitude = 0.56;
        let mut frequency = 1.0;
        let mut norm = 0.0;
        for octave in 0..octaves {
            value +=
                self.value_noise(p * frequency + Vec2::splat(octave as f32 * 17.17)) * amplitude;
            norm += amplitude;
            frequency *= 2.03;
            amplitude *= 0.49;
        }
        value / norm
    }

    fn value_noise(&self, p: Vec2) -> f32 {
        let cell = p.floor();
        let local = p - cell;
        let fade = local * local * (Vec2::splat(3.0) - 2.0 * local);
        let ix = cell.x as i32;
        let iz = cell.y as i32;
        let a = hash_signed(self.seed, ix, iz);
        let b = hash_signed(self.seed, ix + 1, iz);
        let c = hash_signed(self.seed, ix, iz + 1);
        let d = hash_signed(self.seed, ix + 1, iz + 1);
        let x0 = a + (b - a) * fade.x;
        let x1 = c + (d - c) * fade.x;
        x0 + (x1 - x0) * fade.y
    }
}

#[derive(Clone, Copy)]
struct ChunkRequest {
    key: ChunkKey,
    seed: u64,
    peak_intensity: f32,
    visual_style: VisualStyle,
    generation: u64,
    resolution: u32,
}

pub struct TerrainStreamer {
    request_tx: Sender<ChunkRequest>,
    result_rx: Receiver<ChunkMesh>,
    pending: HashSet<ChunkKey>,
    active: HashSet<ChunkKey>,
    incoming: Vec<ChunkMesh>,
    outgoing: Vec<ChunkKey>,
    generation: u64,
    field: TerrainField,
}

impl TerrainStreamer {
    pub fn new(seed: u64, peak_intensity: f32, visual_style: VisualStyle) -> Self {
        let (request_tx, request_rx) = mpsc::channel::<ChunkRequest>();
        let (result_tx, result_rx) = mpsc::channel::<ChunkMesh>();
        thread::Builder::new()
            .name("terrain-generator".into())
            .spawn(move || {
                while let Ok(request) = request_rx.recv() {
                    let field = TerrainField::with_style(
                        request.seed,
                        request.peak_intensity,
                        request.visual_style,
                    );
                    let chunk =
                        generate_chunk(&field, request.key, request.generation, request.resolution);
                    if result_tx.send(chunk).is_err() {
                        break;
                    }
                }
            })
            .expect("terrain worker thread");

        Self {
            request_tx,
            result_rx,
            pending: HashSet::new(),
            active: HashSet::new(),
            incoming: Vec::new(),
            outgoing: Vec::new(),
            generation: 1,
            field: TerrainField::with_style(seed, peak_intensity, visual_style),
        }
    }

    pub fn reset(&mut self, seed: u64, peak_intensity: f32, visual_style: VisualStyle) {
        self.generation = self.generation.wrapping_add(1);
        self.field = TerrainField::with_style(seed, peak_intensity, visual_style);
        self.pending.clear();
        self.outgoing.extend(self.active.drain());
        self.incoming.clear();
    }

    pub fn field(&self) -> TerrainField {
        self.field
    }

    /// A procedural hazard is never allowed to affect physics until its containing terrain chunk
    /// has completed generation and has been handed to the renderer.
    pub fn is_rendered_at(&self, position: Vec3) -> bool {
        [-config::BALL_RADIUS, 0.0, config::BALL_RADIUS]
            .into_iter()
            .all(|offset_x| {
                [-config::BALL_RADIUS, 0.0, config::BALL_RADIUS]
                    .into_iter()
                    .all(|offset_z| {
                        self.active.contains(&ChunkKey::from_world(
                            position.x + offset_x,
                            position.z + offset_z,
                        ))
                    })
            })
    }

    pub fn update(&mut self, player: Vec3, quality: GraphicsQuality) {
        while let Ok(chunk) = self.result_rx.try_recv() {
            if chunk.generation == self.generation {
                self.pending.remove(&chunk.key);
                self.active.insert(chunk.key);
                self.incoming.push(chunk);
            }
        }

        let center = world_chunk(player.x, player.z);
        let radius = match quality {
            GraphicsQuality::Low => 2,
            GraphicsQuality::Medium => 3,
            GraphicsQuality::High => 4,
        };
        let forward_bias = 1;
        let mut wanted = HashSet::new();
        for dz in -radius..=(radius + forward_bias) {
            for dx in -radius..=radius {
                if dx * dx + (dz - forward_bias / 2) * (dz - forward_bias / 2)
                    <= (radius + 1) * (radius + 1)
                {
                    wanted.insert(ChunkKey {
                        x: center.x + dx,
                        z: center.z + dz,
                    });
                }
            }
        }

        let resolution = match quality {
            GraphicsQuality::Low => config::CHUNK_RESOLUTION_LOW,
            GraphicsQuality::Medium => config::CHUNK_RESOLUTION_MEDIUM,
            GraphicsQuality::High => config::CHUNK_RESOLUTION_HIGH,
        };
        for key in wanted.iter().copied() {
            if !self.active.contains(&key) && self.pending.insert(key) {
                let _ = self.request_tx.send(ChunkRequest {
                    key,
                    seed: self.field.seed(),
                    peak_intensity: self.field.peak_intensity,
                    visual_style: self.field.visual_style,
                    generation: self.generation,
                    resolution,
                });
            }
        }

        let stale: Vec<_> = self.active.difference(&wanted).copied().collect();
        for key in stale {
            self.active.remove(&key);
            self.outgoing.push(key);
        }
    }

    pub fn take_changes(&mut self) -> (Vec<ChunkMesh>, Vec<ChunkKey>) {
        (
            std::mem::take(&mut self.incoming),
            std::mem::take(&mut self.outgoing),
        )
    }
}

fn world_chunk(x: f32, z: f32) -> ChunkKey {
    ChunkKey::from_world(x, z)
}

fn generate_chunk(
    field: &TerrainField,
    key: ChunkKey,
    generation: u64,
    resolution: u32,
) -> ChunkMesh {
    let side = resolution + 1;
    let step = config::CHUNK_SIZE / resolution as f32;
    let origin_x = key.x as f32 * config::CHUNK_SIZE;
    let origin_z = key.z as f32 * config::CHUNK_SIZE;
    let mut vertices = Vec::with_capacity((side * side) as usize);
    let mut indices = Vec::with_capacity((resolution * resolution * 6) as usize);

    for z in 0..=resolution {
        for x in 0..=resolution {
            let world_x = origin_x + x as f32 * step;
            let world_z = origin_z + z as f32 * step;
            vertices.push(TerrainVertex {
                position: [world_x, field.height(world_x, world_z), world_z],
                normal: field.normal(world_x, world_z).to_array(),
                tear_info: [field.tear_metric(world_x, world_z).min(3.0), 0.0],
            });
        }
    }
    for z in 0..resolution {
        for x in 0..resolution {
            let a = z * side + x;
            let b = a + 1;
            let c = a + side;
            let d = c + 1;
            let center_x = origin_x + (x as f32 + 0.5) * step;
            let center_z = origin_z + (z as f32 + 0.5) * step;
            // Keep the top mesh continuous and let interpolated distance cut a smooth opening in
            // the fragment shader. Recessed chasm geometry is generated only beneath that cut.
            indices.extend_from_slice(&[a, c, b, b, c, d]);
            if !cell_contains_visible_tear(field, center_x, center_z, step) {
                continue;
            }

            // Recessed floor plus boundary walls makes the gap unmistakable even against the
            // Classic theme's bright fog. Collision deliberately ignores this visual underlay.
            const DEPTH: f32 = 18.0;
            let top = [
                vertices[a as usize].position,
                vertices[b as usize].position,
                vertices[c as usize].position,
                vertices[d as usize].position,
            ];
            let bottom = top.map(|mut point| {
                point[1] -= DEPTH;
                point
            });
            push_tear_quad(&mut vertices, &mut indices, bottom);

            let neighbours = [
                (center_x, center_z - step, 0usize, 1usize),
                (center_x + step, center_z, 1usize, 3usize),
                (center_x, center_z + step, 3usize, 2usize),
                (center_x - step, center_z, 2usize, 0usize),
            ];
            for (check_x, check_z, edge_a, edge_b) in neighbours {
                if !cell_contains_visible_tear(field, check_x, check_z, step) {
                    push_tear_wall(
                        &mut vertices,
                        &mut indices,
                        top[edge_a],
                        top[edge_b],
                        bottom[edge_a],
                        bottom[edge_b],
                    );
                }
            }
        }
    }

    let collectibles = generate_collectibles(field, key);
    ChunkMesh {
        key,
        generation,
        vertices,
        indices,
        collectibles,
    }
}

fn cell_contains_visible_tear(
    field: &TerrainField,
    center_x: f32,
    center_z: f32,
    step: f32,
) -> bool {
    let half = step * 0.5;
    [
        (0.0, 0.0),
        (-half, -half),
        (half, -half),
        (-half, half),
        (half, half),
    ]
    .into_iter()
    .any(|(offset_x, offset_z)| field.is_visible_tear(center_x + offset_x, center_z + offset_z))
}

fn tear_vertex(position: [f32; 3]) -> TerrainVertex {
    TerrainVertex {
        position,
        normal: [0.0, 1.0, 0.0],
        tear_info: [3.0, 1.0],
    }
}

fn push_tear_quad(
    vertices: &mut Vec<TerrainVertex>,
    indices: &mut Vec<u32>,
    points: [[f32; 3]; 4],
) {
    let base = vertices.len() as u32;
    vertices.extend(points.into_iter().map(tear_vertex));
    indices.extend_from_slice(&[base, base + 2, base + 1, base + 1, base + 2, base + 3]);
}

fn push_tear_wall(
    vertices: &mut Vec<TerrainVertex>,
    indices: &mut Vec<u32>,
    top_a: [f32; 3],
    top_b: [f32; 3],
    bottom_a: [f32; 3],
    bottom_b: [f32; 3],
) {
    let base = vertices.len() as u32;
    vertices.extend(
        [top_a, top_b, bottom_a, bottom_b]
            .into_iter()
            .map(tear_vertex),
    );
    // Both windings keep the interior readable from either approach without a second pipeline.
    indices.extend_from_slice(&[
        base,
        base + 1,
        base + 2,
        base + 1,
        base + 3,
        base + 2,
        base + 2,
        base + 1,
        base,
        base + 2,
        base + 3,
        base + 1,
    ]);
}

/// Lays out short lines and arcs, rejecting steep or start-adjacent points. Every candidate is
/// derived only from `(run seed, chunk key, index)` so reloads reproduce the exact route.
fn generate_collectibles(field: &TerrainField, key: ChunkKey) -> Vec<CollectibleSpawn> {
    let base_hash = hash_u64(field.seed(), key.x, key.z);
    let origin = Vec2::new(key.x as f32, key.z as f32) * config::CHUNK_SIZE;
    let route_count = 1 + ((base_hash >> 8) % 2) as usize;
    let mut output = Vec::new();

    for route in 0..route_count {
        let route_hash = mix64(base_hash ^ (route as u64).wrapping_mul(0x9e3779b97f4a7c15));
        let count = 4 + ((route_hash >> 13) % 4) as usize;
        let start = Vec2::new(
            7.0 + unit_float(route_hash) * (config::CHUNK_SIZE - 14.0),
            6.0 + unit_float(route_hash.rotate_left(21)) * (config::CHUNK_SIZE - 12.0),
        );
        let angle = unit_float(route_hash.rotate_left(39)) * std::f32::consts::TAU;
        let direction = Vec2::new(angle.cos(), angle.sin());
        let side = Vec2::new(-direction.y, direction.x);
        let spacing = 2.8;

        for index in 0..count {
            let t = index as f32 - (count - 1) as f32 * 0.5;
            let arc = side * (t * 0.58).sin() * 2.3;
            let p = origin + start + direction * (t * spacing) + arc;
            if p.length() < 18.0
                || field.slope(p.x, p.y) > 0.48
                || field.tear_metric(p.x, p.y) < 2.1
            {
                continue;
            }
            let y = field.height(p.x, p.y) + 1.28;
            let id = mix64(route_hash ^ index as u64);
            output.push(CollectibleSpawn {
                id,
                position: Vec3::new(p.x, y, p.y),
                is_party: mix64(id ^ 0x5041_5254_5921_2121).is_multiple_of(20),
            });
        }
    }
    output
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn hash_signed(seed: u64, x: i32, z: i32) -> f32 {
    unit_float(hash_u64(seed, x, z)) * 2.0 - 1.0
}

fn unit_float(value: u64) -> f32 {
    ((value >> 40) as f32) / ((1u32 << 24) as f32)
}

fn hash_u64(seed: u64, x: i32, z: i32) -> u64 {
    mix64(
        seed ^ (x as i64 as u64).wrapping_mul(0x9e3779b97f4a7c15)
            ^ (z as i64 as u64).rotate_left(32),
    )
}

fn mix64(mut x: u64) -> u64 {
    x ^= x >> 30;
    x = x.wrapping_mul(0xbf58476d1ce4e5b9);
    x ^= x >> 27;
    x = x.wrapping_mul(0x94d049bb133111eb);
    x ^ (x >> 31)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_generation_is_bit_exact() {
        let field = TerrainField::new(0xCAFE_BABE, config::TERRAIN_INTENSITY_DEFAULT);
        let a = generate_chunk(&field, ChunkKey { x: 4, z: -2 }, 1, 18);
        let b = generate_chunk(&field, ChunkKey { x: 4, z: -2 }, 1, 18);
        assert_eq!(
            bytemuck::cast_slice::<_, u8>(&a.vertices),
            bytemuck::cast_slice::<_, u8>(&b.vertices)
        );
        assert_eq!(a.indices, b.indices);
        assert_eq!(a.collectibles.len(), b.collectibles.len());
    }

    #[test]
    fn adjacent_chunk_borders_are_seamless() {
        let field = TerrainField::new(99, config::TERRAIN_INTENSITY_DEFAULT);
        let left = generate_chunk(&field, ChunkKey { x: 0, z: 2 }, 1, 24);
        let right = generate_chunk(&field, ChunkKey { x: 1, z: 2 }, 1, 24);
        let side = 25usize;
        for row in 0..side {
            let a = left.vertices[row * side + side - 1];
            let b = right.vertices[row * side];
            assert_eq!(a.position, b.position);
            assert_eq!(a.normal, b.normal);
        }
    }

    #[test]
    fn collectibles_are_accessible_and_clear_of_start() {
        let field = TerrainField::new(123456, config::TERRAIN_INTENSITY_DEFAULT);
        for z in -4..=4 {
            for x in -4..=4 {
                for item in generate_collectibles(&field, ChunkKey { x, z }) {
                    assert!(item.position.x.hypot(item.position.z) >= 18.0);
                    assert!(field.slope(item.position.x, item.position.z) <= 0.48);
                    assert!(field.tear_metric(item.position.x, item.position.z) >= 2.1);
                    let expected = field.height(item.position.x, item.position.z) + 1.28;
                    assert!((item.position.y - expected).abs() < 0.0001);
                }
            }
        }
    }

    #[test]
    fn tears_are_sparse_deterministic_and_clear_of_the_launch_area() {
        let field = TerrainField::new(0x7EA2_5AFE, config::TERRAIN_INTENSITY_DEFAULT);
        let protected = TerrainField::TEAR_SAFE_RADIUS as i32;
        for z in (-protected..=protected).step_by(3) {
            for x in (-protected..=protected).step_by(3) {
                if (x as f32).hypot(z as f32) <= TerrainField::TEAR_SAFE_RADIUS {
                    assert!(!field.is_tear(x as f32, z as f32));
                    assert!(!field.is_visible_tear(x as f32, z as f32));
                }
            }
        }

        let mut samples = 0usize;
        let mut tears = 0usize;
        let mut visible_opening = 0usize;
        for z in -600..=600 {
            for x in -600..=600 {
                samples += 1;
                tears += usize::from(field.is_tear(x as f32, z as f32));
                visible_opening += usize::from(field.is_visible_tear(x as f32, z as f32));
            }
        }
        let ratio = tears as f32 / samples as f32;
        assert!(
            (0.000005..0.0005).contains(&ratio),
            "tear coverage was {ratio:.4}"
        );
        assert!(
            visible_opening > tears * 3,
            "the visible warning must substantially surround its lethal core"
        );
    }

    #[test]
    fn reported_seeds_have_no_lethal_points_hidden_by_any_quality_mesh() {
        let resolutions = [
            config::CHUNK_RESOLUTION_LOW,
            config::CHUNK_RESOLUTION_MEDIUM,
            config::CHUNK_RESOLUTION_HIGH,
        ];
        for seed in [
            0xBCE1_3BB2_45B4_7C2E,
            0xBDDA_F1A5_038B_DB5F,
            0xBB43_28BA_B873_A1F7,
        ] {
            let field = TerrainField::new(seed, config::TERRAIN_INTENSITY_DEFAULT);
            let mut lethal_points = 0usize;
            for z in (-900..=900).step_by(2) {
                for x in (-900..=900).step_by(2) {
                    let x = x as f32;
                    let z = z as f32;
                    if !field.is_tear(x, z) {
                        continue;
                    }
                    lethal_points += 1;
                    assert!(field.has_visible_tear_warning(x, z));
                    for resolution in resolutions {
                        let rendered = field.rendered_tear_metric(x, z, resolution);
                        assert!(
                            rendered < TerrainField::TEAR_OPENING_METRIC,
                            "seed {seed:016X} lethal point ({x}, {z}) was hidden at resolution {resolution}: {rendered}"
                        );
                    }
                }
            }
            assert!(
                lethal_points > 0,
                "reported seed {seed:016X} should still contain visible, playable tears"
            );
        }
    }

    #[test]
    fn tear_chunks_include_recessed_chasm_geometry() {
        let field = TerrainField::new(0x7EA2_5AFE, config::TERRAIN_INTENSITY_DEFAULT);
        let mut found = false;
        'chunks: for z in -8..=8 {
            for x in -8..=8 {
                let chunk = generate_chunk(&field, ChunkKey { x, z }, 1, 28);
                let has_chasm = chunk
                    .vertices
                    .iter()
                    .any(|vertex| vertex.tear_info[1] > 0.5);
                let has_smooth_cut_data = chunk.vertices.iter().any(|vertex| {
                    vertex.tear_info[1] < 0.5
                        && vertex.tear_info[0] < TerrainField::TEAR_OPENING_METRIC
                });
                if has_chasm && has_smooth_cut_data {
                    found = true;
                    break 'chunks;
                }
            }
        }
        assert!(
            found,
            "test region should contain at least one rendered tear"
        );
    }

    #[test]
    fn peak_setting_increases_relief() {
        let low = TerrainField::new(0x1A7E_4517, 0.70);
        let high = TerrainField::new(0x1A7E_4517, 1.80);
        let points = [
            Vec2::new(180.0, 240.0),
            Vec2::new(-310.0, 125.0),
            Vec2::new(470.0, -280.0),
            Vec2::new(-520.0, -410.0),
        ];
        let low_energy: f32 = points.iter().map(|p| low.height(p.x, p.y).powi(2)).sum();
        let high_energy: f32 = points.iter().map(|p| high.height(p.x, p.y).powi(2)).sum();
        assert!(high_energy > low_energy * 4.0);
    }

    #[test]
    fn default_landscape_contains_distinct_peaks_and_troughs() {
        let field = TerrainField::new(0xD15C_71AC_7EED, config::TERRAIN_INTENSITY_DEFAULT);
        let mut lowest = f32::INFINITY;
        let mut highest = f32::NEG_INFINITY;
        for z in -6..=6 {
            for x in -6..=6 {
                let world_x = x as f32 * 72.0 + 210.0;
                let world_z = z as f32 * 72.0 - 170.0;
                let height = field.height(world_x, world_z);
                lowest = lowest.min(height);
                highest = highest.max(height);
            }
        }
        assert!(lowest < -10.0, "lowest sampled trough was {lowest:.2}");
        assert!(highest > 10.0, "highest sampled peak was {highest:.2}");
        assert!(
            highest - lowest > 32.0,
            "sampled relief was only {:.2}",
            highest - lowest
        );
    }

    #[test]
    fn visual_profiles_change_relief_and_bank_character() {
        let seed = 0x0A11_CE5C_49E5;
        let intensity = config::TERRAIN_INTENSITY_DEFAULT;
        let classic = TerrainField::with_style(seed, intensity, VisualStyle::Classic);
        let vaporwave = TerrainField::with_style(seed, intensity, VisualStyle::Vaporwave);
        let dark = TerrainField::with_style(seed, intensity, VisualStyle::Dark);

        let stats = |field: TerrainField| {
            let mut lowest = f32::INFINITY;
            let mut highest = f32::NEG_INFINITY;
            let mut steepest: f32 = 0.0;
            for z in -7..=7 {
                for x in -7..=7 {
                    let world_x = x as f32 * 68.0 + 215.0;
                    let world_z = z as f32 * 68.0 - 185.0;
                    let height = field.height(world_x, world_z);
                    lowest = lowest.min(height);
                    highest = highest.max(height);
                    steepest = steepest.max(field.slope(world_x, world_z));
                }
            }
            (highest - lowest, steepest)
        };

        let classic_stats = stats(classic);
        let vaporwave_stats = stats(vaporwave);
        let dark_stats = stats(dark);
        assert!(vaporwave_stats.0 > classic_stats.0 * 1.05);
        assert!(dark_stats.1 > classic_stats.1 * 1.05);
    }

    #[test]
    fn party_pickups_are_rare_but_present() {
        let field = TerrainField::new(0x0050_4152_5459, config::TERRAIN_INTENSITY_DEFAULT);
        let mut total = 0usize;
        let mut parties = 0usize;
        for z in -12..=12 {
            for x in -12..=12 {
                for item in generate_collectibles(&field, ChunkKey { x, z }) {
                    total += 1;
                    parties += usize::from(item.is_party);
                }
            }
        }
        let ratio = parties as f32 / total as f32;
        assert!(
            (0.025..=0.075).contains(&ratio),
            "party ratio was {ratio:.3}"
        );
    }
}
