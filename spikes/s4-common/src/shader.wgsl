// SPDX-License-Identifier: MIT OR Apache-2.0
// Spike S4 viewport shader: shaded mesh, draped pass preview, gradient background.

struct Uniforms {
    view_proj: mat4x4<f32>,
    eye: vec4<f32>,
    base: vec4<f32>,      // model colour, linear
    accent: vec4<f32>,    // preview line colour, linear
    preview: vec4<f32>,   // stepover mm, angle rad, enabled (0/1), unused
    bg_top: vec4<f32>,
    bg_bottom: vec4<f32>,
};

@group(0) @binding(0) var<uniform> u: Uniforms;

// The target is a plain RGBA8 texture that the toolkits sample without
// conversion, so the shader writes sRGB-encoded values.
fn to_srgb(c: vec3<f32>) -> vec3<f32> {
    let lo = c * 12.92;
    let hi = 1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(hi, lo, c <= vec3<f32>(0.0031308));
}

struct MeshOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) normal: vec3<f32>,
};

@vertex
fn vs_mesh(@location(0) pos: vec3<f32>, @location(1) normal: vec3<f32>) -> MeshOut {
    var out: MeshOut;
    out.clip = u.view_proj * vec4<f32>(pos, 1.0);
    out.world = pos;
    out.normal = normal;
    return out;
}

@fragment
fn fs_mesh(in: MeshOut) -> @location(0) vec4<f32> {
    let n = normalize(in.normal);
    let v = normalize(u.eye.xyz - in.world);
    // A fixed key light, a weak head light and a sky/ground hemisphere fill.
    let key = normalize(vec3<f32>(-0.5, -0.7, 0.6));
    let diffuse = max(dot(n, key), 0.0) * 0.65 + max(dot(n, v), 0.0) * 0.2;
    let fill = mix(0.05, 0.25, 0.5 + 0.5 * n.z);
    let h = normalize(key + v);
    let spec = pow(max(dot(n, h), 0.0), 48.0) * 0.12;
    var col = u.base.rgb * (diffuse + fill) + vec3<f32>(spec);

    // Draped preview of raster passes (CH-23): lines at the stepover across
    // the pass angle, about one pixel wide at any zoom.
    let dir = vec2<f32>(cos(u.preview.y), sin(u.preview.y));
    let s = dot(in.world.xy, vec2<f32>(-dir.y, dir.x)) / max(u.preview.x, 1e-3);
    let w = fwidth(s);
    let f = abs(fract(s + 0.5) - 0.5);
    let line = (1.0 - smoothstep(0.5 * w, 1.5 * w, f)) * u.preview.z;
    col = mix(col, u.accent.rgb, line * 0.9);
    return vec4<f32>(to_srgb(col), 1.0);
}

struct BgOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) t: f32,
};

@vertex
fn vs_bg(@builtin(vertex_index) i: u32) -> BgOut {
    // One triangle covering the screen.
    let x = f32(i32(i & 1u) * 4 - 1);
    let y = f32(i32(i >> 1u) * 4 - 1);
    var out: BgOut;
    out.clip = vec4<f32>(x, y, 0.0, 1.0);
    out.t = 0.5 + 0.5 * y;
    return out;
}

@fragment
fn fs_bg(in: BgOut) -> @location(0) vec4<f32> {
    return vec4<f32>(to_srgb(mix(u.bg_bottom.rgb, u.bg_top.rgb, clamp(in.t, 0.0, 1.0))), 1.0);
}
