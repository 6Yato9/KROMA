// Log. Slots:
//   0 mode, 1 preview_influence, 2 strength, 3 pivot,
//   4 hue_angle, 5 protect_neutrals,
//   6 shadow_strength, 7 shadow_hue, 8 highlight_strength, 9 highlight_hue
//
// Opposing hues into shadows and highlights — orange highlights against teal
// shadows being the obvious one. Perceptual, so it belongs in log.
//
// Defaults follow Resolve: strength 0.5, pivot 0.3, hue angle 20. That means
// adding this effect changes the image immediately, which is deliberate on
// their part and worth matching: a look effect that does nothing until you
// touch it reads as broken.

/// Where "neutral enough to leave alone" ends and colour begins.
///
/// Below the first number a pixel is treated as grey and takes no tint at all;
/// above the second it takes the full amount. Skin sits comfortably above
/// both, which is the point — Protect Neutrals is for concrete and overcast
/// sky, not for faces.
const PROTECT_FROM: f32 = 0.04;
const PROTECT_TO: f32 = 0.18;

const MODE_NATURAL: f32 = 0.0;
const MODE_STRONG: f32 = 1.0;
const MODE_CUSTOM: f32 = 2.0;

fn effect(c: vec3<f32>, uv: vec2<f32>) -> vec3<f32> {
    let mode = u.p[0].x;
    let preview = u.p[0].y > 0.5;
    let strength = u.p[0].z;
    let pivot = u.p[0].w;
    let hue_angle = u.p[1].x;
    let protect = u.p[1].y > 0.5;

    if strength <= 0.0 {
        return c;
    }

    let l = clamp(luma(c), 0.0, 1.0);

    // Signed position either side of the pivot: -1 at black, 0 at the pivot,
    // +1 at white. Normalising each side separately is what lets the pivot sit
    // off-centre (0.3 by default) without squashing one side.
    var t: f32;
    if l < pivot {
        t = (l - pivot) / max(pivot, 1e-4);
    } else {
        t = (l - pivot) / max(1.0 - pivot, 1e-4);
    }
    t = clamp(t, -1.0, 1.0);

    // Shadows take the opposing hue, 180 degrees round the wheel.
    var shadow_hue = fract((hue_angle + 180.0) / 360.0);
    var highlight_hue = fract(hue_angle / 360.0);
    var shadow_amount = strength;
    var highlight_amount = strength;

    if mode >= MODE_CUSTOM - 0.5 {
        // Custom decouples the two ends completely.
        shadow_amount = strength * u.p[1].z;
        shadow_hue = fract(u.p[1].w / 360.0);
        highlight_amount = strength * u.p[2].x;
        highlight_hue = fract(u.p[2].y / 360.0);
    }

    let is_highlight = t > 0.0;
    let hue = select(shadow_hue, highlight_hue, is_highlight);
    var amount = select(shadow_amount, highlight_amount, is_highlight) * abs(t);

    // Natural rolls the tint off towards the extremes so the brightest point
    // stays white, mimicking film. Strong carries colour all the way up, which
    // is the stylised look.
    if mode < MODE_STRONG - 0.5 {
        amount = amount * (1.0 - abs(t) * 0.55);
    }

    // Protect Neutrals keeps low-saturation regions grey. Without it, a split
    // tone tints skin and concrete alike.
    //
    // The band is fixed rather than exposed. Resolve shows a bare checkbox
    // here, and the two sliders we used to have were a second way of asking
    // the same question the checkbox already answers.
    if protect {
        let s = rgb_to_hsv(c).y;
        amount = amount * smoothstep(PROTECT_FROM, PROTECT_TO, s);
    }

    let tint = hsv_to_rgb(vec3<f32>(hue, 1.0, 1.0));

    if preview {
        // Shows which colour lands where, over a desaturated image, so the
        // split point is visible rather than guessed at.
        return mix(vec3<f32>(l), tint, clamp(abs(t) * 1.5, 0.0, 1.0));
    }

    // Move toward the hue while holding luminance, so the split tone changes
    // colour without also changing exposure.
    let tinted = c * (vec3<f32>(1.0) + tint - vec3<f32>(luma(tint)));
    return mix(c, tinted, clamp(amount * 0.6, 0.0, 1.0));
}
