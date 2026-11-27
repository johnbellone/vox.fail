// Retro sprite mode: shows the small render of the avatar with big square pixels, every pixel
// snapped to the character's palette (three shades per colour), plus a one-pixel dark outline
// around the silhouette. See src/stage.rs.
import bevy_ui_render::ui_vertex_output::UiVertexOutput;

struct RetroParams {
    palette: array<vec4<f32>, 256>,
    count: u32,
    size: vec2<u32>,
    outline: vec4<f32>,
}

@group(1) @binding(0) var<uniform> params: RetroParams;
@group(1) @binding(1) var image: texture_2d<f32>;

fn linear_to_srgb(c: vec3<f32>) -> vec3<f32> {
    let lo = c * 12.92;
    let hi = 1.055 * pow(max(c, vec3<f32>(0.0)), vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(hi, lo, c <= vec3<f32>(0.0031308));
}

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((c + 0.055) / 1.055, vec3<f32>(2.4));
    return select(hi, lo, c <= vec3<f32>(0.04045));
}

fn inside(p: vec2<i32>) -> bool {
    let q = clamp(p, vec2<i32>(0), vec2<i32>(params.size) - vec2<i32>(1));
    return textureLoad(image, q, 0).a > 0.5;
}

@fragment
fn fragment(in: UiVertexOutput) -> @location(0) vec4<f32> {
    let p = vec2<i32>(floor(in.uv * vec2<f32>(params.size)));
    let c = textureLoad(image, p, 0);
    if c.a > 0.5 {
        // the render was cleared to transparent black, so edge pixels are premultiplied
        let s = linear_to_srgb(c.rgb / max(c.a, 0.001));
        var best = 0u;
        var best_d = 1e9;
        for (var i = 0u; i < params.count; i = i + 1u) {
            let d = s - params.palette[i].rgb;
            let dd = dot(d * d, vec3<f32>(0.30, 0.59, 0.11));
            if dd < best_d {
                best_d = dd;
                best = i;
            }
        }
        return vec4<f32>(srgb_to_linear(params.palette[best].rgb), 1.0);
    }
    if inside(p + vec2<i32>(1, 0)) || inside(p - vec2<i32>(1, 0)) || inside(p + vec2<i32>(0, 1)) || inside(p - vec2<i32>(0, 1)) {
        return params.outline;
    }
    return vec4<f32>(0.0);
}
