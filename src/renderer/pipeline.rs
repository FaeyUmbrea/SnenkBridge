pub const WGSL_SHADER_SOURCE: &str = r#"
struct Uniforms {
    cam_dist: f32,
    focal: f32,
    _pad0: f32,
    _pad1: f32,
};

@group(0) @binding(0)
var<uniform> uniforms: Uniforms;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) view_position: vec3<f32>,
    @location(1) normal: vec3<f32>,
};

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.view_position = input.position;
    out.normal = input.normal;

    let view_z = input.position.z;
    let perspective_scale = uniforms.focal / max(uniforms.cam_dist - view_z, 0.1);
    let ndc_x = input.position.x * perspective_scale * 2.0;
    let ndc_y = input.position.y * perspective_scale * 2.0;
    let ndc_z = clamp((view_z / 0.1) * 0.5 + 0.5, 0.0, 1.0);

    out.clip_position = vec4<f32>(ndc_x, ndc_y, ndc_z, 1.0);
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    var normal = in.normal;
    let len_sq = dot(normal, normal);
    if (len_sq > 1e-6) {
        normal = normalize(normal);
    } else {
        normal = vec3<f32>(0.0, 0.0, 1.0);
    }

    let key_light_dir = normalize(vec3<f32>(0.35, 0.55, 0.75));
    let fill_light_dir = normalize(vec3<f32>(-0.45, -0.2, 0.6));
    let half_vector = normalize(vec3<f32>(key_light_dir.x, key_light_dir.y, key_light_dir.z + 1.0));

    let diffuse_key = max(dot(normal, key_light_dir), 0.0);
    let diffuse_fill = max(dot(normal, fill_light_dir), 0.0);
    let specular_dot = max(dot(normal, half_vector), 0.0);
    let specular_highlight = pow(specular_dot, 14.0);
    let rim_lighting = pow(1.0 - clamp(normal.z, 0.0, 1.0), 2.0);

    let color_r = (36.0 + diffuse_key * 170.0 + diffuse_fill * 55.0 + specular_highlight * 140.0 + rim_lighting * 35.0) / 255.0;
    let color_g = (34.0 + diffuse_key * 155.0 + diffuse_fill * 70.0 + specular_highlight * 140.0 + rim_lighting * 30.0) / 255.0;
    let color_b = (44.0 + diffuse_key * 175.0 + diffuse_fill * 95.0 + specular_highlight * 160.0 + rim_lighting * 45.0) / 255.0;

    return vec4<f32>(clamp(color_r, 0.0, 1.0), clamp(color_g, 0.0, 1.0), clamp(color_b, 0.0, 1.0), 1.0);
}
"#;

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
}

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ShaderUniforms {
    pub cam_dist: f32,
    pub focal: f32,
    pub _pad0: f32,
    pub _pad1: f32,
}
