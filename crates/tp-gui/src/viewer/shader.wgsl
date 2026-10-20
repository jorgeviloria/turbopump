// Shading of the impeller in the 3D view.
//
// Lighting is computed in linear RGB and written gamma-encoded, because egui samples
// user textures as plain (not sRGB-aware) Rgba8Unorm.

struct Uniforms {
    view_projection: mat4x4<f32>,
    eye: vec4<f32>,
};

@group(0) @binding(0) var<uniform> u: Uniforms;

struct VertexIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec4<f32>,
};

struct VertexOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec4<f32>,
};

@vertex
fn vs_main(v: VertexIn) -> VertexOut {
    var out: VertexOut;
    out.clip = u.view_projection * vec4<f32>(v.position, 1.0);
    out.world = v.position;
    out.normal = v.normal;
    out.color = v.color;
    return out;
}

fn gamma_from_linear(c: vec3<f32>) -> vec3<f32> {
    let cutoff = c < vec3<f32>(0.0031308);
    let lower = c * 12.92;
    let higher = 1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(higher, lower, cutoff);
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let to_eye = normalize(u.eye.xyz - in.world);
    var n = normalize(in.normal);
    // Open surfaces: light both faces.
    if dot(n, to_eye) < 0.0 {
        n = -n;
    }
    // A head light slightly above the eye, plus a weak fill light from below.
    let key = to_eye;
    let diffuse = max(dot(n, key), 0.0);
    let half_vector = normalize(key + to_eye);
    let specular = pow(max(dot(n, half_vector), 0.0), 60.0) * 0.35;
    let rim = pow(1.0 - max(dot(n, to_eye), 0.0), 3.0) * 0.15;
    let rgb = in.color.rgb * (0.18 + 0.82 * diffuse) + vec3<f32>(specular + rim);
    return vec4<f32>(gamma_from_linear(clamp(rgb, vec3<f32>(0.0), vec3<f32>(1.0))), in.color.a);
}

@fragment
fn fs_line(in: VertexOut) -> @location(0) vec4<f32> {
    return vec4<f32>(gamma_from_linear(in.color.rgb), 1.0);
}
