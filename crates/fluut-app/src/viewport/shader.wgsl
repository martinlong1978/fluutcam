// SPDX-License-Identifier: MIT OR Apache-2.0
// FluutCAM viewport (D13 §5): shaded faces, lines (edges, stock, toolpaths),
// gradient background, and the ID pass for picking (§5.2).

struct Uniforms {
    view_proj: mat4x4<f32>,
    eye: vec4<f32>,
    base: vec4<f32>,       // face colour, linear
    accent: vec4<f32>,     // selection colour, linear
    bg_top: vec4<f32>,
    bg_bottom: vec4<f32>,
    palette: array<vec4<f32>, 8>, // line colours by tag
    rest: array<vec4<f32>, 4>,    // stock by rest class: off, rest, within, gouge
    clips: array<vec4<f32>, 8>,   // stock by toolpath
    // x: picked face index + 1 (0 = none)
    // y: stock colouring: 0 plain, 1 rest class, 2 toolpath
    selection: vec4<u32>,
};

// Per draw call (dynamic offset).
struct Draw {
    // x: brightness of lines; zw: shift of lines, in clip space
    style: vec4<f32>,
    // x: pick id of the draw (lines); faces encode their own
    id: vec4<u32>,
    // rgb: tint of a highlight redraw; a: how much of it
    tint: vec4<f32>,
};

@group(0) @binding(0) var<uniform> u: Uniforms;
@group(1) @binding(0) var<uniform> d: Draw;

const FACE_LAYER: u32 = 0x10000000u;
const EDGE_LAYER: u32 = 0x30000000u;
// Stock vertices: this bit, then the tag (class in bits 16-17, toolpath + 1
// in bits 0-15).
const STOCK_BIT: u32 = 0x80000000u;

// The target is a plain RGBA8 texture that egui samples without conversion,
// so the shader writes sRGB-encoded values.
fn to_srgb(c: vec3<f32>) -> vec3<f32> {
    let lo = c * 12.92;
    let hi = 1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(hi, lo, c <= vec3<f32>(0.0031308));
}

struct MeshOut {
    // Invariant: a highlight redraws a face over itself at equal depth.
    @invariant @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) @interpolate(flat) face: u32,
};

@vertex
fn vs_mesh(
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) face: u32,
) -> MeshOut {
    var out: MeshOut;
    out.clip = u.view_proj * vec4<f32>(pos, 1.0);
    out.world = pos;
    out.normal = normal;
    out.face = face;
    return out;
}

@fragment
fn fs_mesh(in: MeshOut) -> @location(0) vec4<f32> {
    let v = normalize(u.eye.xyz - in.world);
    // Two-sided: light the side facing the eye.
    var n = normalize(in.normal);
    n = select(n, -n, dot(n, v) < 0.0);
    let key = normalize(vec3<f32>(-0.5, -0.7, 0.6));
    let diffuse = max(dot(n, key), 0.0) * 0.6 + max(dot(n, v), 0.0) * 0.25;
    let fill = mix(0.08, 0.25, 0.5 + 0.5 * n.z);
    let h = normalize(key + v);
    let spec = pow(max(dot(n, h), 0.0), 48.0) * 0.12;
    var base = u.base.rgb;
    if ((in.face & STOCK_BIT) != 0u) {
        let rest = (in.face >> 16u) & 3u;
        let tp = in.face & 0xFFFFu;
        if (u.selection.y == 1u && rest != 0u) {
            base = u.rest[rest].rgb;
        } else if (u.selection.y == 2u && tp != 0u) {
            base = u.clips[(tp - 1u) % 7u].rgb;
        }
    } else if (in.face + 1u == u.selection.x) {
        base = mix(base, u.accent.rgb, 0.6);
    }
    base = mix(base, d.tint.rgb, d.tint.a);
    let col = base * (diffuse + fill) + vec3<f32>(spec);
    return vec4<f32>(to_srgb(col), 1.0);
}

@fragment
fn fs_mesh_pick(in: MeshOut) -> @location(0) u32 {
    return FACE_LAYER | in.face;
}

struct LineOut {
    @invariant @builtin(position) clip: vec4<f32>,
    @location(0) colour: vec3<f32>,
    @location(1) @interpolate(flat) pick: u32,
};

@vertex
fn vs_line(
    @location(0) pos: vec3<f32>,
    @location(1) tag: u32,
    @location(2) pick: u32,
) -> LineOut {
    var out: LineOut;
    out.clip = u.view_proj * vec4<f32>(pos, 1.0);
    // Pull lines a little toward the eye so edges on faces stay visible.
    out.clip.z = out.clip.z - 0.0004 * out.clip.w;
    let lit = u.palette[min(tag, 7u)].rgb * d.style.x;
    out.colour = mix(lit, d.tint.rgb, d.tint.a);
    out.clip = vec4<f32>(out.clip.xy + d.style.zw * out.clip.w, out.clip.zw);
    out.pick = pick;
    return out;
}

@fragment
fn fs_line(in: LineOut) -> @location(0) vec4<f32> {
    return vec4<f32>(to_srgb(in.colour), 1.0);
}

@fragment
fn fs_line_pick(in: LineOut) -> @location(0) u32 {
    // Edges carry their own index; the stock box in their buffer has none.
    if (in.pick != 0u) {
        return EDGE_LAYER | (in.pick - 1u);
    }
    if (d.id.x == 0u) {
        discard;
    }
    return d.id.x;
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
