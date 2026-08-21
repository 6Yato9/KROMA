// Appended to every effect shader. Owns the colour-space bookkeeping and the
// row blend, so no individual effect has to think about either.

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) idx: u32) -> VsOut {
    var out: VsOut;
    let x = f32((idx << 1u) & 2u);
    let y = f32(idx & 2u);
    out.uv = vec2<f32>(x, y);
    out.pos = vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let src = textureSampleLevel(src_texture, src_sampler, in.uv, 0.0);
    let base_linear = src.rgb;
    let is_log = u.space_is_log == 1u;

    // Into the space the effect declared.
    var base = base_linear;
    if is_log {
        base = cct_encode(base_linear);
    }

    let result = effect(base, in.uv);

    var out_linear: vec3<f32>;
    if is_log && blend_is_light_like(u.blend_mode) {
        // Add and Screen model light summing at a sensor, so they are
        // evaluated in linear even when the effect itself works in log.
        out_linear = blend(base_linear, cct_decode(result), u.blend_mode, u.opacity);
    } else {
        let mixed = blend(base, result, u.blend_mode, u.opacity);
        if is_log {
            out_linear = cct_decode(mixed);
        } else {
            out_linear = mixed;
        }
    }

    return vec4<f32>(out_linear, src.a);
}
