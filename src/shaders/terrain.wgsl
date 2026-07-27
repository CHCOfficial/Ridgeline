struct SceneUniform {
    view_proj: mat4x4<f32>,
    camera_position: vec4<f32>,
    ball_position_radius: vec4<f32>,
    sun_direction_time: vec4<f32>,
    fog_color: vec4<f32>,
    party: vec4<f32>,
    visual_style: vec4<f32>,
    trail_info: vec4<f32>,
    trail_marks: array<vec4<f32>, 64>,
};

@group(0) @binding(0) var<uniform> scene: SceneUniform;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) tear_info: vec2<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) distance_to_camera: f32,
    @location(3) trail_imprint: f32,
    @location(4) tear_info: vec2<f32>,
};

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var position = input.position;
    var trail_deformation = 0.0;
    var index = 0u;
    loop {
        if index >= 64u || f32(index) >= scene.trail_info.y {
            break;
        }
        let mark = scene.trail_marks[index];
        let offset = position.xz - mark.xy;
        let normalized_distance = dot(offset, offset) / max(mark.z * mark.z, 0.0001);
        trail_deformation += exp(-normalized_distance * 2.45) * mark.w * scene.trail_info.x;
        index += 1u;
    }
    let bounded_deformation = min(trail_deformation, 0.62);
    position.y -= bounded_deformation;
    var output: VertexOutput;
    output.clip_position = scene.view_proj * vec4<f32>(position, 1.0);
    output.world_position = position;
    output.normal = input.normal;
    output.distance_to_camera = distance(scene.camera_position.xyz, input.position);
    output.trail_imprint = bounded_deformation / 0.62;
    output.tear_info = input.tear_info;
    return output;
}

fn grid_line(coordinate: vec2<f32>) -> f32 {
    let grid_coordinate = coordinate / 1.62;
    let width = fwidth(grid_coordinate);
    let distance_to_line = abs(fract(grid_coordinate - 0.5) - 0.5) / max(width, vec2<f32>(0.0001));
    return 1.0 - smoothstep(0.56, 1.18, min(distance_to_line.x, distance_to_line.y));
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let top_surface = 1.0 - step(0.5, input.tear_info.y);
    if top_surface > 0.5 && input.tear_info.x < 1.24 {
        discard;
    }
    let tear_rim = top_surface * (1.0 - smoothstep(1.24, 2.35, input.tear_info.x));
    let normal = normalize(input.normal);
    let light_direction = normalize(-scene.sun_direction_time.xyz);
    let diffuse = max(dot(normal, light_direction), 0.0);
    let sky = 0.50 + 0.50 * normal.y;
    let slope_ao = mix(0.46, 1.0, smoothstep(0.28, 0.98, normal.y));
    let elevation_light = mix(0.50, 1.10, smoothstep(-15.0, 14.0, input.world_position.y));
    let local_height = input.world_position.y - (scene.ball_position_radius.y - scene.ball_position_radius.w);
    let valley_light = mix(0.46, 1.0, smoothstep(-20.0, 5.0, local_height));
    let landscape_light = 0.50 + diffuse * 0.42 + sky * 0.07;
    var surface = vec3<f32>(0.89, 0.895, 0.902) * landscape_light * slope_ao * valley_light * elevation_light;

    // Style profiles keep the same legible contour language while changing the world itself.
    // Vaporwave favours luminous crests over deep violet bowls; Dark exaggerates bank-facing
    // light so its steeper geometry reads as carved charcoal rather than a flat recolour.
    let vapor_height = smoothstep(-18.0, 18.0, input.world_position.y);
    let vapor_band = 0.5 + 0.5 * sin(input.world_position.x * 0.026 + input.world_position.z * 0.018);
    let vapor_low = mix(vec3<f32>(0.030, 0.006, 0.090), vec3<f32>(0.125, 0.015, 0.205), vapor_band);
    let vapor_high = mix(vec3<f32>(0.30, 0.025, 0.40), vec3<f32>(0.035, 0.20, 0.31), vapor_band);
    let vapor_surface = mix(vapor_low, vapor_high, vapor_height)
        * (0.66 + diffuse * 0.48 + sky * 0.10)
        * mix(0.55, 1.0, slope_ao)
        * mix(0.58, 1.06, valley_light);

    let dark_height = smoothstep(-22.0, 20.0, input.world_position.y);
    let bank_edge = pow(1.0 - normal.y, 0.58);
    let dark_base = mix(vec3<f32>(0.012, 0.013, 0.017), vec3<f32>(0.105, 0.108, 0.118), dark_height);
    let dark_surface = dark_base * (0.50 + diffuse * 0.76 + sky * 0.06)
        + vec3<f32>(0.075, 0.078, 0.086) * bank_edge * (0.22 + diffuse * 0.70);

    surface = mix(surface, vapor_surface, scene.visual_style.x);
    surface = mix(surface, dark_surface, scene.visual_style.y);
    surface *= 1.0 - input.trail_imprint * 0.12;

    // Every tear carries a high-contrast animated RGB perimeter during hazard hardening. Spatial
    // phase offsets make the spectrum travel along the edge, while the faster sparkle term keeps
    // it visibly shimmering even when the ball or a steep bank partially occludes the opening.
    let tear_phase = scene.sun_direction_time.w * 4.2
        + input.world_position.x * 0.21
        + input.world_position.z * 0.29
        + input.tear_info.x * 3.7;
    let tear_rgb = vec3<f32>(0.52) + vec3<f32>(0.48) * cos(
        vec3<f32>(tear_phase, tear_phase + 2.094, tear_phase + 4.189)
    );
    let tear_shimmer = 0.74 + 0.26 * sin(
        scene.sun_direction_time.w * 12.0
        + input.world_position.x * 1.7
        - input.world_position.z * 1.3
    );
    surface = mix(
        surface,
        tear_rgb * (1.18 + tear_shimmer * 0.62),
        tear_rim * (0.78 + tear_shimmer * 0.22),
    );
    let chasm_color = mix(vec3<f32>(0.008, 0.005, 0.006), vec3<f32>(0.028, 0.002, 0.045), scene.visual_style.x);
    surface = mix(surface, chasm_color, input.tear_info.y);

    let party_wave = 0.5 + 0.5 * sin(input.world_position.x * 0.075 + input.world_position.z * 0.045 + scene.party.y * 1.8);
    let vapor_tint = mix(vec3<f32>(1.03, 0.91, 1.01), vec3<f32>(0.90, 1.02, 1.04), party_wave);
    surface = mix(surface, surface * vapor_tint, scene.party.x * 0.10);

    let grid = grid_line(input.world_position.xz);
    let grid_fade = 1.0 - smoothstep(95.0, 205.0, input.distance_to_camera);
    let party_grid = mix(vec3<f32>(0.08, 0.82, 0.90), vec3<f32>(0.94, 0.08, 0.60), party_wave);
    var grid_color = vec3<f32>(0.26, 0.275, 0.30);
    let vapor_grid = mix(vec3<f32>(0.02, 0.86, 1.0), vec3<f32>(1.0, 0.025, 0.62), party_wave);
    let dark_grid = mix(vec3<f32>(0.30, 0.31, 0.34), vec3<f32>(0.70, 0.71, 0.74), diffuse);
    grid_color = mix(grid_color, vapor_grid, scene.visual_style.x);
    grid_color = mix(grid_color, dark_grid, scene.visual_style.y);
    grid_color = mix(grid_color, party_grid, scene.party.x * 0.20);
    let style_grid_strength = 0.39 + scene.visual_style.x * 0.37 + scene.visual_style.y * 0.20;
    let grid_strength = mix(style_grid_strength, style_grid_strength + 0.05, scene.party.x);
    surface = mix(
        surface,
        grid_color,
        grid * grid_strength * grid_fade * (1.0 - input.tear_info.y) * (1.0 - tear_rim * 0.88),
    );

    // A soft terrain-following contact shadow gives an immediate grounded/airborne cue. The
    // height gate makes it recede naturally whenever the sphere leaves the surface.
    let shadow_offset = normalize(scene.sun_direction_time.xz) * scene.ball_position_radius.w * 0.28;
    let ball_delta = input.world_position.xz - (scene.ball_position_radius.xz + shadow_offset);
    let contact_distance = length(ball_delta) / max(scene.ball_position_radius.w, 0.01);
    let contact_height = scene.ball_position_radius.y - scene.ball_position_radius.w;
    let height_alignment = 1.0 - smoothstep(0.15, 4.0, abs(contact_height - input.world_position.y));
    let shadow_core = exp(-contact_distance * contact_distance * 1.65) * 0.38;
    let shadow_soft = exp(-contact_distance * contact_distance * 0.24) * 0.24;
    surface *= 1.0 - (shadow_core + shadow_soft) * height_alignment;

    // Tears keep their silhouette through the bright distance fog. The lethal core is narrower
    // than this dark opening, so the player always receives a clear visual warning first.
    let tear_fog_protection = clamp(input.tear_info.y * 0.98 + tear_rim * 0.96, 0.0, 0.99);
    let fog = smoothstep(120.0, 285.0, input.distance_to_camera) * (1.0 - tear_fog_protection);
    surface = mix(surface, scene.fog_color.rgb, fog);
    return vec4<f32>(surface, 1.0);
}
