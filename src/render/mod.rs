use crate::{
    config,
    game::{hue_to_rgb, Game},
    persistence::{TrailStyle, VisualStyle},
    terrain::{ChunkKey, ChunkMesh, TerrainVertex},
};
use bytemuck::{Pod, Zeroable};
use egui::TexturesDelta;
use egui_wgpu::{Renderer as EguiRenderer, ScreenDescriptor};
use glam::{Mat3, Mat4, Quat, Vec3, Vec3Swizzles};
use std::{collections::HashMap, mem, sync::Arc};
use wgpu::util::DeviceExt;
use winit::{dpi::PhysicalSize, window::Window};

const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
const SAMPLE_COUNT: u32 = 4;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct SceneUniform {
    view_proj: [[f32; 4]; 4],
    camera_position: [f32; 4],
    ball_position_radius: [f32; 4],
    sun_direction_time: [f32; 4],
    fog_color: [f32; 4],
    party: [f32; 4],
    visual_style: [f32; 4],
    trail_info: [f32; 4],
    trail_marks: [[f32; 4]; config::TRAIL_DEFORMATION_MARKS],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct SphereVertex {
    position: [f32; 3],
    normal: [f32; 3],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct InstanceRaw {
    model_0: [f32; 4],
    model_1: [f32; 4],
    model_2: [f32; 4],
    model_3: [f32; 4],
    color: [f32; 4],
}

impl InstanceRaw {
    fn new(position: Vec3, rotation: Quat, scale: f32, color: [f32; 4]) -> Self {
        Self::new_scaled(position, rotation, Vec3::splat(scale), color)
    }

    fn new_scaled(position: Vec3, rotation: Quat, scale: Vec3, color: [f32; 4]) -> Self {
        let matrix =
            Mat4::from_scale_rotation_translation(scale, rotation, position).to_cols_array_2d();
        Self {
            model_0: matrix[0],
            model_1: matrix[1],
            model_2: matrix[2],
            model_3: matrix[3],
            color,
        }
    }
}

struct GpuChunk {
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    index_count: u32,
}

struct TextureTarget {
    _texture: wgpu::Texture,
    view: wgpu::TextureView,
}

pub struct Renderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    size: PhysicalSize<u32>,
    uniform_buffer: wgpu::Buffer,
    uniform_bind_group: wgpu::BindGroup,
    terrain_pipeline: wgpu::RenderPipeline,
    sphere_pipeline: wgpu::RenderPipeline,
    sphere_vertex_buffer: wgpu::Buffer,
    sphere_index_buffer: wgpu::Buffer,
    sphere_index_count: u32,
    instance_buffer: wgpu::Buffer,
    instance_capacity: usize,
    chunks: HashMap<ChunkKey, GpuChunk>,
    depth: TextureTarget,
    multisample: TextureTarget,
    egui: EguiRenderer,
}

impl Renderer {
    pub async fn new(window: Arc<Window>) -> Result<Self, String> {
        let size = window.inner_size();
        let instance = wgpu::Instance::default();
        let surface = instance
            .create_surface(window)
            .map_err(|error| error.to_string())?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .ok_or_else(|| "No compatible graphics adapter was found".to_owned())?;
        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("ridgeline-device"),
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits::default(),
                },
                None,
            )
            .await
            .map_err(|error| error.to_string())?;
        let capabilities = surface.get_capabilities(&adapter);
        let format = capabilities
            .formats
            .iter()
            .copied()
            .find(wgpu::TextureFormat::is_srgb)
            .unwrap_or(capabilities.formats[0]);
        let present_mode = if capabilities
            .present_modes
            .contains(&wgpu::PresentMode::AutoVsync)
        {
            wgpu::PresentMode::AutoVsync
        } else {
            wgpu::PresentMode::Fifo
        };
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode,
            desired_maximum_frame_latency: 2,
            alpha_mode: capabilities.alpha_modes[0],
            view_formats: vec![],
        };
        surface.configure(&device, &config);

        let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("scene-uniform"),
            contents: bytemuck::bytes_of(&SceneUniform::zeroed()),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let uniform_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("scene-uniform-layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let uniform_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("scene-uniform-group"),
            layout: &uniform_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("scene-pipeline-layout"),
            bind_group_layouts: &[&uniform_layout],
            push_constant_ranges: &[],
        });
        let terrain_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("terrain-shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/terrain.wgsl").into()),
        });
        let sphere_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("sphere-shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/sphere.wgsl").into()),
        });

        let common_primitive = wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            strip_index_format: None,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: Some(wgpu::Face::Back),
            polygon_mode: wgpu::PolygonMode::Fill,
            unclipped_depth: false,
            conservative: false,
        };
        let depth_stencil = Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: true,
            depth_compare: wgpu::CompareFunction::Less,
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        });
        let terrain_color_target = Some(wgpu::ColorTargetState {
            format,
            blend: Some(wgpu::BlendState::REPLACE),
            write_mask: wgpu::ColorWrites::ALL,
        });
        let sphere_color_target = Some(wgpu::ColorTargetState {
            format,
            blend: Some(wgpu::BlendState::ALPHA_BLENDING),
            write_mask: wgpu::ColorWrites::ALL,
        });

        let terrain_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("terrain-pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &terrain_shader,
                entry_point: "vs_main",
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: mem::size_of::<TerrainVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2],
                }],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &terrain_shader,
                entry_point: "fs_main",
                targets: &[terrain_color_target],
                compilation_options: Default::default(),
            }),
            primitive: common_primitive,
            depth_stencil: depth_stencil.clone(),
            multisample: wgpu::MultisampleState {
                count: SAMPLE_COUNT,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            multiview: None,
        });
        let sphere_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("sphere-pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &sphere_shader,
                entry_point: "vs_main",
                buffers: &[
                    wgpu::VertexBufferLayout {
                        array_stride: mem::size_of::<SphereVertex>() as u64,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3],
                    },
                    wgpu::VertexBufferLayout {
                        array_stride: mem::size_of::<InstanceRaw>() as u64,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes: &wgpu::vertex_attr_array![2 => Float32x4, 3 => Float32x4, 4 => Float32x4, 5 => Float32x4, 6 => Float32x4],
                    },
                ],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &sphere_shader,
                entry_point: "fs_main",
                targets: &[sphere_color_target],
                compilation_options: Default::default(),
            }),
            primitive: common_primitive,
            depth_stencil,
            multisample: wgpu::MultisampleState { count: SAMPLE_COUNT, mask: !0, alpha_to_coverage_enabled: false },
            multiview: None,
        });

        let (sphere_vertices, sphere_indices) = make_uv_sphere(28, 18);
        let sphere_vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("sphere-vertices"),
            contents: bytemuck::cast_slice(&sphere_vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let sphere_index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("sphere-indices"),
            contents: bytemuck::cast_slice(&sphere_indices),
            usage: wgpu::BufferUsages::INDEX,
        });
        let instance_capacity = 4096;
        let instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("sphere-instances"),
            size: (instance_capacity * mem::size_of::<InstanceRaw>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let depth = create_target(&device, size, DEPTH_FORMAT, SAMPLE_COUNT, "depth");
        let multisample = create_target(&device, size, format, SAMPLE_COUNT, "multisample-color");
        let egui = EguiRenderer::new(&device, format, None, 1);

        Ok(Self {
            surface,
            device,
            queue,
            config,
            size,
            uniform_buffer,
            uniform_bind_group,
            terrain_pipeline,
            sphere_pipeline,
            sphere_vertex_buffer,
            sphere_index_buffer,
            sphere_index_count: sphere_indices.len() as u32,
            instance_buffer,
            instance_capacity,
            chunks: HashMap::new(),
            depth,
            multisample,
            egui,
        })
    }

    pub fn resize(&mut self, size: PhysicalSize<u32>) {
        if size.width == 0 || size.height == 0 {
            return;
        }
        self.size = size;
        self.config.width = size.width;
        self.config.height = size.height;
        self.surface.configure(&self.device, &self.config);
        self.depth = create_target(&self.device, size, DEPTH_FORMAT, SAMPLE_COUNT, "depth");
        self.multisample = create_target(
            &self.device,
            size,
            self.config.format,
            SAMPLE_COUNT,
            "multisample-color",
        );
    }

    pub fn sync_terrain(&mut self, incoming: Vec<ChunkMesh>, outgoing: Vec<ChunkKey>) {
        for key in outgoing {
            self.chunks.remove(&key);
        }
        for chunk in incoming {
            let vertex_buffer = self
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("terrain-chunk-vertices"),
                    contents: bytemuck::cast_slice(&chunk.vertices),
                    usage: wgpu::BufferUsages::VERTEX,
                });
            let index_buffer = self
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("terrain-chunk-indices"),
                    contents: bytemuck::cast_slice(&chunk.indices),
                    usage: wgpu::BufferUsages::INDEX,
                });
            self.chunks.insert(
                chunk.key,
                GpuChunk {
                    vertex_buffer,
                    index_buffer,
                    index_count: chunk.indices.len() as u32,
                },
            );
        }
    }

    pub fn render(
        &mut self,
        game: &Game,
        interpolation: f32,
        camera_zoom: f32,
        paint_jobs: &[egui::ClippedPrimitive],
        textures: &TexturesDelta,
        pixels_per_point: f32,
    ) -> Result<(), wgpu::SurfaceError> {
        let (ball_position, ball_rotation) = game.interpolated_ball(interpolation);
        let (base_camera_position, base_camera_target) = game.camera.interpolated(interpolation);
        let camera_delta = base_camera_position - base_camera_target;
        let (camera_position, camera_target, style_view_scale) = match game.visual_style {
            VisualStyle::Classic => (base_camera_position, base_camera_target, 1.0),
            VisualStyle::Vaporwave => (
                base_camera_target
                    + Vec3::new(
                        camera_delta.x * 1.08,
                        camera_delta.y * 0.80,
                        camera_delta.z * 1.08,
                    ),
                base_camera_target - Vec3::Y * 0.8,
                1.08,
            ),
            VisualStyle::Dark => (
                base_camera_target
                    + Vec3::new(
                        camera_delta.x * 1.28,
                        camera_delta.y * 0.66,
                        camera_delta.z * 1.28,
                    ),
                base_camera_target - Vec3::Y * 2.4,
                1.12,
            ),
        };
        let aspect = self.config.width as f32 / self.config.height as f32;
        let half_height =
            config::CAMERA_VIEW_HEIGHT * camera_zoom.clamp(0.72, 1.55) * style_view_scale * 0.5;
        let half_width = half_height * aspect;
        let projection = Mat4::orthographic_rh(
            -half_width,
            half_width,
            -half_height,
            half_height,
            0.15,
            430.0,
        );
        let view = Mat4::look_at_rh(camera_position, camera_target, Vec3::Y);
        let party_amount = if game.party_active() { 1.0 } else { 0.0 };
        let trail_center = ChunkKey::from_world(ball_position.x, ball_position.z);
        let mut visible_surface_trail = Vec::new();
        for z in (trail_center.z - 2)..=(trail_center.z + 2) {
            for x in (trail_center.x - 2)..=(trail_center.x + 2) {
                if let Some(points) = game.surface_trail.get(&ChunkKey { x, z }) {
                    visible_surface_trail.extend(points.iter().filter(|point| {
                        point.position.xz().distance_squared(ball_position.xz()) < 82.0 * 82.0
                    }));
                }
            }
        }
        let mut deformation_points: Vec<_> = visible_surface_trail
            .iter()
            .copied()
            .filter(|point| point.deformation && game.trail_deformation)
            .collect();
        deformation_points.sort_unstable_by_key(|point| std::cmp::Reverse(point.sequence));
        deformation_points.truncate(config::TRAIL_DEFORMATION_MARKS);
        let mut trail_marks = [[0.0; 4]; config::TRAIL_DEFORMATION_MARKS];
        for (mark, point) in trail_marks.iter_mut().zip(&deformation_points) {
            *mark = [point.position.x, point.position.z, 1.14, 0.21];
        }
        // PARTY remains anchored to the player so terrain relief and pickups stay readable.
        let (fog_color, visual_style, sun_direction) = match game.visual_style {
            VisualStyle::Classic => (
                [0.965, 0.968, 0.972, 1.0],
                [0.0, 0.0, 0.0, 0.0],
                [-0.42, -0.82, -0.34, game.elapsed],
            ),
            VisualStyle::Vaporwave => (
                [0.040, 0.010, 0.105, 1.0],
                [1.0, 0.0, 0.0, 0.0],
                [-0.52, -0.64, -0.56, game.elapsed],
            ),
            VisualStyle::Dark => (
                [0.010, 0.011, 0.014, 1.0],
                [0.0, 1.0, 0.0, 0.0],
                [-0.68, -0.54, -0.46, game.elapsed],
            ),
        };
        let uniform = SceneUniform {
            view_proj: (projection * view).to_cols_array_2d(),
            camera_position: camera_position.extend(1.0).to_array(),
            ball_position_radius: ball_position.extend(config::BALL_RADIUS).to_array(),
            sun_direction_time: sun_direction,
            fog_color,
            party: [party_amount, game.elapsed, game.party_time, 0.0],
            visual_style,
            trail_info: [
                if game.trail_deformation { 1.0 } else { 0.0 },
                deformation_points.len() as f32,
                0.0,
                0.0,
            ],
            trail_marks,
        };
        self.queue
            .write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(&uniform));

        let mut instances = Vec::with_capacity(
            1 + game.collectibles.len() * 2
                + game.particles.len()
                + game.trail.len() * 2
                + visible_surface_trail.len() * 3,
        );
        let pulse_scale = config::BALL_RADIUS * (1.0 + game.ball.pulse * 0.12);
        let player_color = if game.party_active() {
            let rgb = hue_to_rgb((game.elapsed * 0.22).fract());
            [rgb[0], rgb[1], rgb[2], 2.2]
        } else {
            match game.visual_style {
                VisualStyle::Classic => [0.91, 0.018, 0.025, 0.18],
                VisualStyle::Vaporwave => [0.92, 0.30, 0.94, 2.2],
                VisualStyle::Dark => [0.98, 0.006, 0.018, 0.42],
            }
        };
        instances.push(InstanceRaw::new(
            ball_position,
            ball_rotation,
            pulse_scale,
            player_color,
        ));
        for item in game.collectibles.values() {
            let bob = (game.elapsed * 2.3 + item.phase).sin() * 0.14;
            let spin = Quat::from_rotation_y(game.elapsed * 1.3 + item.phase);
            let color = if item.is_party {
                let rgb =
                    hue_to_rgb((game.elapsed * 0.28 + item.phase / std::f32::consts::TAU) % 1.0);
                [rgb[0], rgb[1], rgb[2], 1.85]
            } else {
                match game.visual_style {
                    VisualStyle::Classic => [0.94, 0.60, 0.08, 0.38],
                    VisualStyle::Vaporwave => [0.02, 0.92, 1.0, 1.44],
                    VisualStyle::Dark => [0.96, 0.20, 0.10, 1.12],
                }
            };
            instances.push(InstanceRaw::new(
                item.position + Vec3::Y * bob,
                spin,
                if item.is_party { 0.36 } else { 0.33 },
                color,
            ));
            if item.is_party {
                instances.push(InstanceRaw::new(
                    item.position + Vec3::Y * bob,
                    spin,
                    0.56 + (game.elapsed * 4.0 + item.phase).sin() * 0.04,
                    [color[0], color[1], color[2], 3.0],
                ));
            }
        }
        for point in visible_surface_trail {
            let travel = Vec3::new(point.direction.x, 0.0, point.direction.y);
            let mut forward =
                (travel - point.normal * travel.dot(point.normal)).normalize_or_zero();
            if forward.length_squared() < 0.5 {
                forward = Vec3::Z;
            }
            let right = point.normal.cross(forward).normalize_or_zero();
            let rotation = Quat::from_mat3(&Mat3::from_cols(right, point.normal, forward));
            let embedded_position = point.position
                + point.normal
                    * if point.deformation && game.trail_deformation {
                        -0.042
                    } else {
                        0.014
                    };
            if point.deformation
                && game.trail_deformation
                && !matches!(point.style, TrailStyle::Off | TrailStyle::Smoke)
            {
                instances.push(InstanceRaw::new_scaled(
                    embedded_position - point.normal * 0.012,
                    rotation,
                    Vec3::new(0.22, 0.032, 0.40),
                    [0.13, 0.14, 0.16, 0.05],
                ));
            }
            match point.style {
                TrailStyle::Off => {}
                TrailStyle::Smoke => {
                    let scatter = point.sequence as f32 * 2.399_963_1;
                    let size = 0.026 + (scatter * 1.7).sin().abs() * 0.014;
                    let position = point.position
                        + point.normal * (0.018 + size * 0.35)
                        + right * scatter.sin() * 0.055;
                    instances.push(InstanceRaw::new(
                        position,
                        Quat::IDENTITY,
                        size,
                        [0.69, 0.71, 0.74, -0.13],
                    ));
                }
                TrailStyle::Graphite => instances.push(InstanceRaw::new_scaled(
                    embedded_position,
                    rotation,
                    Vec3::new(0.105, 0.020, 0.31),
                    [0.075, 0.08, 0.095, 0.08],
                )),
                TrailStyle::Neon => {
                    instances.push(InstanceRaw::new_scaled(
                        embedded_position,
                        rotation,
                        Vec3::new(0.105, 0.020, 0.32),
                        [0.01, 0.78, 1.0, 1.62],
                    ));
                    instances.push(InstanceRaw::new_scaled(
                        embedded_position + point.normal * 0.012,
                        rotation,
                        Vec3::new(0.18, 0.026, 0.40),
                        [0.01, 0.72, 1.0, 3.0],
                    ));
                }
                TrailStyle::Prism => {
                    let rgb = hue_to_rgb(point.hue);
                    instances.push(InstanceRaw::new_scaled(
                        embedded_position,
                        rotation,
                        Vec3::new(0.12, 0.021, 0.30),
                        [rgb[0], rgb[1], rgb[2], 1.56],
                    ));
                    instances.push(InstanceRaw::new_scaled(
                        embedded_position + point.normal * 0.012,
                        rotation,
                        Vec3::new(0.19, 0.027, 0.39),
                        [rgb[0], rgb[1], rgb[2], 3.0],
                    ));
                }
            }
        }
        for point in &game.trail {
            let life = (1.0 - point.age / point.lifetime).clamp(0.0, 1.0);
            let rgb = hue_to_rgb((point.hue + game.elapsed * 0.035).fract());
            let scale = config::BALL_RADIUS * (0.07 + life * 0.29);
            instances.push(InstanceRaw::new(
                point.position,
                Quat::IDENTITY,
                scale,
                [rgb[0], rgb[1], rgb[2], 1.45 + life * 0.35],
            ));
            instances.push(InstanceRaw::new(
                point.position,
                Quat::IDENTITY,
                scale * 1.62,
                [rgb[0], rgb[1], rgb[2], 3.0],
            ));
        }
        for particle in &game.particles {
            let life = 1.0 - particle.age / particle.lifetime;
            instances.push(InstanceRaw::new(
                particle.position,
                Quat::IDENTITY,
                0.095 * life.max(0.08),
                [
                    particle.color[0],
                    particle.color[1],
                    particle.color[2],
                    0.72,
                ],
            ));
        }
        self.ensure_instance_capacity(instances.len());
        self.queue
            .write_buffer(&self.instance_buffer, 0, bytemuck::cast_slice(&instances));

        for (id, delta) in &textures.set {
            self.egui
                .update_texture(&self.device, &self.queue, *id, delta);
        }
        let frame = self.surface.get_current_texture()?;
        let surface_view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame-encoder"),
            });
        let screen = ScreenDescriptor {
            size_in_pixels: [self.config.width, self.config.height],
            pixels_per_point,
        };
        let user_commands =
            self.egui
                .update_buffers(&self.device, &self.queue, &mut encoder, paint_jobs, &screen);

        let clear_color = wgpu::Color {
            r: fog_color[0] as f64,
            g: fog_color[1] as f64,
            b: fog_color[2] as f64,
            a: 1.0,
        };
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("scene-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.multisample.view,
                    resolve_target: Some(&surface_view),
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(clear_color),
                        store: wgpu::StoreOp::Discard,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth.view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_bind_group(0, &self.uniform_bind_group, &[]);
            pass.set_pipeline(&self.terrain_pipeline);
            for chunk in self.chunks.values() {
                pass.set_vertex_buffer(0, chunk.vertex_buffer.slice(..));
                pass.set_index_buffer(chunk.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..chunk.index_count, 0, 0..1);
            }
            if !instances.is_empty() {
                pass.set_pipeline(&self.sphere_pipeline);
                pass.set_vertex_buffer(0, self.sphere_vertex_buffer.slice(..));
                pass.set_vertex_buffer(1, self.instance_buffer.slice(..));
                pass.set_index_buffer(
                    self.sphere_index_buffer.slice(..),
                    wgpu::IndexFormat::Uint32,
                );
                pass.draw_indexed(0..self.sphere_index_count, 0, 0..instances.len() as u32);
            }
        }

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("ui-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &surface_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            self.egui.render(&mut pass, paint_jobs, &screen);
        }

        self.queue.submit(
            user_commands
                .into_iter()
                .chain(std::iter::once(encoder.finish())),
        );
        frame.present();
        for id in &textures.free {
            self.egui.free_texture(id);
        }
        Ok(())
    }

    fn ensure_instance_capacity(&mut self, required: usize) {
        if required <= self.instance_capacity {
            return;
        }
        self.instance_capacity = required.next_power_of_two();
        self.instance_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("sphere-instances"),
            size: (self.instance_capacity * mem::size_of::<InstanceRaw>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
    }
}

fn create_target(
    device: &wgpu::Device,
    size: PhysicalSize<u32>,
    format: wgpu::TextureFormat,
    sample_count: u32,
    label: &str,
) -> TextureTarget {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: size.width.max(1),
            height: size.height.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    TextureTarget {
        _texture: texture,
        view,
    }
}

fn make_uv_sphere(longitudes: u32, latitudes: u32) -> (Vec<SphereVertex>, Vec<u32>) {
    let mut vertices = Vec::with_capacity(((longitudes + 1) * (latitudes + 1)) as usize);
    let mut indices = Vec::with_capacity((longitudes * latitudes * 6) as usize);
    for latitude in 0..=latitudes {
        let v = latitude as f32 / latitudes as f32;
        let theta = v * std::f32::consts::PI;
        for longitude in 0..=longitudes {
            let u = longitude as f32 / longitudes as f32;
            let phi = u * std::f32::consts::TAU;
            let normal = Vec3::new(
                theta.sin() * phi.cos(),
                theta.cos(),
                theta.sin() * phi.sin(),
            );
            vertices.push(SphereVertex {
                position: normal.to_array(),
                normal: normal.to_array(),
            });
        }
    }
    let side = longitudes + 1;
    for latitude in 0..latitudes {
        for longitude in 0..longitudes {
            let a = latitude * side + longitude;
            let b = a + side;
            indices.extend_from_slice(&[a, b, a + 1, a + 1, b, b + 1]);
        }
    }
    (vertices, indices)
}
