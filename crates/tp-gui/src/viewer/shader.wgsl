// Shading of the 3D view.
//
// Lighting is computed in linear RGB and written gamma-encoded, because egui samples
// user textures as plain (not sRGB-aware) Rgba8Unorm. The background colors arrive
// already gamma-encoded.

struct Uniforms {
    view_projection: mat4x4<f32>,
    // The eye, or with w = 0 the direction toward it in a parallel view.
    eye: vec4<f32>,
    // Directions toward the key and fill lights, world space.
    key_light: vec4<f32>,
    fill_light: vec4<f32>,
    // Screen up direction, world space, for the sky-and-ground ambient.
    up: vec4<f32>,
    // Background at the top and at the bottom of the view.
    sky: vec4<f32>,
    ground: vec4<f32>,
    // The cutting plane, normal · p = w, the side the normal points to removed; a zero
    // normal when the model is whole.
    clip: vec4<f32>,
    // Gamma-encoded tint of the part under the pointer and of the one selected.
    accent: vec4<f32>,
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

fn transform(v: VertexIn) -> VertexOut {
    var out: VertexOut;
    out.clip = u.view_projection * vec4<f32>(v.position, 1.0);
    out.world = v.position;
    out.normal = v.normal;
    out.color = v.color;
    return out;
}

@vertex
fn vs_main(v: VertexIn) -> VertexOut {
    return transform(v);
}

// Lines lie on the surfaces they outline: pull them a little toward the eye so the
// surface does not hide them.
@vertex
fn vs_line(v: VertexIn) -> VertexOut {
    var out = transform(v);
    out.clip.z = out.clip.z - 0.0006 * out.clip.w;
    return out;
}

fn cut_away(p: vec3<f32>) -> bool {
    return dot(u.clip.xyz, u.clip.xyz) > 0.25 && dot(u.clip.xyz, p) > u.clip.w;
}

fn gamma_from_linear(c: vec3<f32>) -> vec3<f32> {
    let cutoff = c < vec3<f32>(0.0031308);
    let lower = c * 12.92;
    let higher = 1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(higher, lower, cutoff);
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    if cut_away(in.world) {
        discard;
    }
    let to_eye = select(normalize(u.eye.xyz - in.world), normalize(u.eye.xyz), u.eye.w == 0.0);
    var n = normalize(in.normal);
    // Open surfaces: light both faces.
    if dot(n, to_eye) < 0.0 {
        n = -n;
    }
    let key = normalize(u.key_light.xyz);
    let fill = normalize(u.fill_light.xyz);
    let sky = 0.5 + 0.5 * dot(n, normalize(u.up.xyz));
    let ambient = mix(0.12, 0.26, sky);
    let diffuse = 0.72 * max(dot(n, key), 0.0) + 0.22 * max(dot(n, fill), 0.0);
    let half_vector = normalize(key + to_eye);
    // Metallic highlight, tinted by the base color.
    let specular = pow(max(dot(n, half_vector), 0.0), 56.0) * 0.5;
    let facing = max(dot(n, to_eye), 0.0);
    let rim = pow(1.0 - facing, 4.0);
    let base = in.color.rgb;
    let rgb = base * (ambient + diffuse)
        + specular * mix(vec3<f32>(1.0), base * 2.0, 0.4)
        + vec3<f32>(rim * 0.08);
    // Glass shows more at grazing angles.
    let alpha = select(in.color.a, clamp(in.color.a * (1.0 + 2.0 * rim), 0.0, 0.85), in.color.a < 1.0);
    return vec4<f32>(gamma_from_linear(clamp(rgb, vec3<f32>(0.0), vec3<f32>(1.0))), alpha);
}

@fragment
fn fs_line(in: VertexOut) -> @location(0) vec4<f32> {
    if cut_away(in.world) {
        discard;
    }
    return vec4<f32>(gamma_from_linear(in.color.rgb), 1.0);
}

struct HighlightOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) @interpolate(flat) strength: f32,
};

// A part drawn again over itself in the accent color: instance 0 is the part under
// the pointer, instance 1 the selected one, more strongly.
@vertex
fn vs_highlight(v: VertexIn, @builtin(instance_index) i: u32) -> HighlightOut {
    var out: HighlightOut;
    out.clip = u.view_projection * vec4<f32>(v.position, 1.0);
    // A hair toward the eye, so the tint wins the depth test on its own surface.
    out.clip.z = out.clip.z - 0.0002 * out.clip.w;
    out.world = v.position;
    out.strength = select(0.22, 0.4, i == 1u);
    return out;
}

@fragment
fn fs_highlight(in: HighlightOut) -> @location(0) vec4<f32> {
    if cut_away(in.world) {
        discard;
    }
    return vec4<f32>(u.accent.rgb, in.strength);
}

struct BackgroundOut {
    @builtin(position) clip: vec4<f32>,
    // 0 at the bottom of the view, 1 at the top.
    @location(0) height: f32,
};

// One triangle that covers the whole view.
@vertex
fn vs_background(@builtin(vertex_index) i: u32) -> BackgroundOut {
    let x = f32((i << 1u) & 2u);
    let y = f32(i & 2u);
    var out: BackgroundOut;
    out.clip = vec4<f32>(2.0 * x - 1.0, 2.0 * y - 1.0, 1.0, 1.0);
    out.height = y;
    return out;
}

@fragment
fn fs_background(in: BackgroundOut) -> @location(0) vec4<f32> {
    let t = smoothstep(0.0, 1.0, clamp(in.height, 0.0, 1.0));
    return vec4<f32>(mix(u.ground.rgb, u.sky.rgb, t), 1.0);
}
