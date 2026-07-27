struct SceneUniform {
    view_proj: mat4x4<f32>,
    camera_position: vec4<f32>,
    ball_position_radius: vec4<f32>,
    sun_direction_time: vec4<f32>,
    fog_color: vec4<f32>,
    party: vec4<f32>,
};

@group(0) @binding(0) var<uniform> scene: SceneUniform;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) model_0: vec4<f32>,
    @location(3) model_1: vec4<f32>,
    @location(4) model_2: vec4<f32>,
    @location(5) model_3: vec4<f32>,
    @location(6) color: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec4<f32>,
};

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    let model = mat4x4<f32>(input.model_0, input.model_1, input.model_2, input.model_3);
    let world = model * vec4<f32>(input.position, 1.0);
    // Inverse-transpose for the renderer's rotation + non-uniform scale matrices. Expressing it
    // through the squared column lengths keeps this compatible with wgpu's WGSL feature level.
    let transformed_normal = model[0].xyz * input.normal.x / max(dot(model[0].xyz, model[0].xyz), 0.00001)
        + model[1].xyz * input.normal.y / max(dot(model[1].xyz, model[1].xyz), 0.00001)
        + model[2].xyz * input.normal.z / max(dot(model[2].xyz, model[2].xyz), 0.00001);
    var output: VertexOutput;
    output.clip_position = scene.view_proj * world;
    output.world_position = world.xyz;
    output.normal = normalize(transformed_normal);
    output.color = input.color;
    return output;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let normal = normalize(input.normal);
    let light_direction = normalize(-scene.sun_direction_time.xyz);
    let view_direction = normalize(scene.camera_position.xyz - input.world_position);
    let half_vector = normalize(light_direction + view_direction);
    let diffuse = max(dot(normal, light_direction), 0.0);
    let specular = pow(max(dot(normal, half_vector), 0.0), 54.0) * (0.35 + input.color.a);
    let rim = pow(1.0 - max(dot(normal, view_direction), 0.0), 3.0) * 0.18;
    if input.color.a < 0.0 {
        let smoke_light = 0.72 + diffuse * 0.20;
        return vec4<f32>(input.color.rgb * smoke_light, abs(input.color.a));
    }
    if input.color.a > 2.75 {
        let glow = pow(1.0 - max(dot(normal, view_direction), 0.0), 1.7);
        return vec4<f32>(input.color.rgb * (1.2 + glow * 1.8), 0.035 + glow * 0.22);
    }
    if input.color.a > 2.0 {
        let vapor_band = 0.5 + 0.5 * sin(normal.y * 5.2 + normal.x * 2.4 + scene.party.y * 3.1);
        let vapor = mix(vec3<f32>(1.0, 0.025, 0.58), vec3<f32>(0.02, 0.92, 1.0), vapor_band);
        let shifting_color = mix(vapor, input.color.rgb, 0.34);
        let vapor_rim = pow(1.0 - max(dot(normal, view_direction), 0.0), 2.0);
        let lit = shifting_color * (0.62 + diffuse * 0.34);
        return vec4<f32>(lit + shifting_color * 0.48 + vec3<f32>(specular) + vec3<f32>(vapor_rim * 0.34), 1.0);
    }
    let base = input.color.rgb * (0.48 + diffuse * 0.48);
    let emissive = max(input.color.a - 1.0, 0.0);
    let party_rim = vec3<f32>(0.05, 0.8, 1.0) * rim * scene.party.x * step(1.0, input.color.a) * 1.5;
    let color = base + vec3<f32>(specular) + vec3<f32>(rim) + input.color.rgb * emissive * 0.65 + party_rim;
    return vec4<f32>(color, 1.0);
}
