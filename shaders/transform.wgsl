// Colour-space transform pass.
//
// Used twice at M0, once at each end of the pipeline:
//
//   input:  Rgba8UnormSrgb source  ->  Rgba16Float working (ACEScg)
//   output: Rgba16Float working    ->  Bgra8UnormSrgb surface
//
// Only the *gamut* rotation happens here. Both transfer functions are done by
// the hardware for free: sampling an `...Srgb` texture applies the sRGB EOTF,
// and writing to an `...Srgb` render target applies the OETF. Doing them in the
// shader as well would apply them twice, which is the classic washed-out or
// crushed-looking result.

struct Transform {
    // Gamut rotation. Column-major with 16-byte stride; pe_color::Mat3::to_wgsl_mat3
    // produces exactly this layout.
    gamut: mat3x3<f32>,
    // Where to read from, as an affine map from output uv to source uv.
    // Everything geometric composes into this one map — the crop, the
    // straightening angle, the quarter turns and flips, and the preview's own
    // zoom and pan — so the source is sampled exactly once no matter how many
    // of them are in play. Resampling per operation would soften the picture a
    // little for each one, for nothing.
    //
    // axes.xy is where a step along the output's x lands; axes.zw the same for
    // y; origin.xy is where output (0, 0) reads.
    axes: vec4<f32>,
    // xy: the origin. z: whether to blank samples that fall outside the source
    // rather than smearing the edge pixel across them.
    origin: vec4<f32>,
}

@group(0) @binding(0) var src_texture: texture_2d<f32>;
@group(0) @binding(1) var src_sampler: sampler;
@group(0) @binding(2) var<uniform> xf: Transform;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

// A single oversized triangle covering the viewport. Cheaper than two triangles
// and avoids the diagonal seam artefacts a quad can produce with some
// interpolation modes.
@vertex
fn vs_fullscreen(@builtin(vertex_index) idx: u32) -> VsOut {
    var out: VsOut;
    let x = f32((idx << 1u) & 2u);
    let y = f32(idx & 2u);
    out.uv = vec2<f32>(x, y);
    out.pos = vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
    return out;
}

@fragment
fn fs_transform(in: VsOut) -> @location(0) vec4<f32> {
    let uv = xf.origin.xy + xf.axes.xy * in.uv.x + xf.axes.zw * in.uv.y;

    // Straightening turns the picture inside a rectangle that no longer fits
    // it, so the corners have nothing behind them. Black says that plainly;
    // the sampler's clamp-to-edge would instead smear the outermost row of
    // pixels outwards, which reads as a real part of the photograph and makes
    // it hard to see where the image actually ends.
    if xf.origin.z > 0.5
        && (uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0) {
        return vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }

    let c = textureSample(src_texture, src_sampler, uv);
    // No clamping. Values outside 0..1 are legitimate here — highlights above
    // diffuse white, and negative channels where a wide-gamut colour does not
    // fit the destination. Clamping now would bake in a hue shift before gamut
    // mapping has had a chance to handle it properly.
    return vec4<f32>(xf.gamut * c.rgb, c.a);
}
