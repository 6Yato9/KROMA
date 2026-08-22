// Log. Slots:
//   0 stock        1 strength
//   2 highlight_rolloff  3 shadow_rolloff  4 film_contrast  5 film_saturation
//   6 shadow_hue   7 shadow_tone   8 highlight_hue   9 highlight_tone
//
// Slots follow the order the parameters are declared in the registry, so that
// order is load-bearing and this comment has to move with it.
//
// This is the *response* half of Resolve's Film Look Creator: what a stock
// does to the light before anything is added to the picture. The other half —
// halation, grain, bloom, vignette — is a set of rows in this application
// already, and writing them again inside here would mean two implementations
// of halation that drift apart the first time one is fixed.
//
// Log, because everything here is about how the picture reads rather than how
// much light there was. A film response curve is a perceptual object: it is
// the shape of the print, not a photometric measurement.

/// Where the shoulder and the toe sit, as fractions of the SDR range.
///
/// Film has no clipping point — density keeps rising all the way up, ever more
/// slowly — which is why a highlight on film rolls off instead of stopping.
/// Reproducing that shoulder is most of what makes a digital picture read as
/// film, and it is the part a LUT usually gets and a set of sliders usually
/// misses.
const FILM_SPAN: f32 = CCT_WHITE - CCT_BLACK;

struct Stock {
    /// How hard the shoulder and toe bend.
    shoulder: f32,
    toe: f32,
    /// Contrast through the middle, and how much colour the stock holds.
    contrast: f32,
    saturation: f32,
}

/// The three stocks, as the four numbers that separate them.
///
/// Not presets that write the sliders: choosing one changes the *base* the
/// sliders modulate, so a stock is still a stock after you have adjusted it.
/// Presets that overwrite your work are a worse control than a menu.
fn stock_of(index: f32) -> Stock {
    var s: Stock;
    let i = i32(round(index));
    switch i {
        // A modern colour negative: long shoulder, gentle toe, restrained.
        case 0: {
            s.shoulder = 0.55;
            s.toe = 0.35;
            s.contrast = 1.06;
            s.saturation = 1.02;
        }
        // A saturated consumer stock: shorter shoulder, more contrast, more
        // colour — the look people mean when they say "film".
        case 1: {
            s.shoulder = 0.75;
            s.toe = 0.50;
            s.contrast = 1.18;
            s.saturation = 1.16;
        }
        // Reversal: very short shoulder and a hard toe, which is why slide
        // film clips highlights and blocks shadows the way it does.
        default: {
            s.shoulder = 1.05;
            s.toe = 0.85;
            s.contrast = 1.30;
            s.saturation = 1.10;
        }
    }
    return s;
}

/// Roll one end of the range off asymptotically.
///
/// Exponential, so nothing ever reaches the limit. That is the property that
/// matters: a shoulder that *arrives* somewhere is a clip with extra steps,
/// and the whole reason film highlights look the way they do is that they
/// never quite get there.
fn rolloff(v: f32, knee: f32, amount: f32, upward: bool) -> f32 {
    if amount <= 0.0 {
        return v;
    }
    let room = max((1.0 - knee) * (0.35 + amount * 0.9), 1e-4);
    if upward {
        let over = max(v - knee, 0.0);
        return min(v, knee) + room * (1.0 - exp(-over / room));
    }
    let under = max(knee - v, 0.0);
    return max(v, knee) - room * (1.0 - exp(-under / room));
}

fn effect(c: vec3<f32>, uv: vec2<f32>) -> vec3<f32> {
    let stock = stock_of(slot(0u));
    let strength = clamp(slot(1u), 0.0, 1.0);
    let highlight_rolloff = max(slot(2u), 0.0) * stock.shoulder;
    let shadow_rolloff = max(slot(3u), 0.0) * stock.toe;
    let film_contrast = slot(4u) * stock.contrast;
    let film_saturation = slot(5u) * stock.saturation;
    let shadow_hue = slot(6u);
    let shadow_tone = slot(7u);
    let highlight_hue = slot(8u);
    let highlight_tone = slot(9u);

    if strength <= 0.0 {
        return c;
    }

    // Work in the fraction of the range a photograph occupies, so the knees
    // sit where black and white are rather than at arbitrary log values.
    var t = (c - vec3<f32>(CCT_BLACK)) / FILM_SPAN;

    // Contrast about mid grey, which in this normalised range is where 18%
    // lands.
    let mid = (CCT_GREY - CCT_BLACK) / FILM_SPAN;
    t = vec3<f32>(mid) + (t - vec3<f32>(mid)) * film_contrast;

    // The shoulder and the toe. Per channel rather than on luminance, because
    // that is what film does — each dye layer saturates on its own, and it is
    // why a red that clips on film goes orange rather than white.
    t = vec3<f32>(
        rolloff(t.r, 0.72, highlight_rolloff, true),
        rolloff(t.g, 0.72, highlight_rolloff, true),
        rolloff(t.b, 0.72, highlight_rolloff, true),
    );
    t = vec3<f32>(
        rolloff(t.r, 0.14, shadow_rolloff, false),
        rolloff(t.g, 0.14, shadow_rolloff, false),
        rolloff(t.b, 0.14, shadow_rolloff, false),
    );

    var out = vec3<f32>(CCT_BLACK) + t * FILM_SPAN;

    // How much colour the stock holds.
    if abs(film_saturation - 1.0) > 1e-4 {
        var hsv = rgb_to_hsv(out);
        hsv.y = clamp(hsv.y * film_saturation, 0.0, 8.0);
        out = hsv_to_rgb(hsv);
    }

    // Split toning, weighted the way the print is: colour in the shadows and
    // in the highlights, nothing through the middle.
    let l = luma(out);
    let shadow_w = 1.0 - smoothstep(CCT_BLACK, CCT_GREY, l);
    let highlight_w = smoothstep(CCT_GREY, CCT_WHITE, l);
    if shadow_tone > 0.0 {
        let tint = hsv_to_rgb(vec3<f32>(fract(shadow_hue / 360.0), 1.0, 1.0));
        // Zero-mean, so toning shifts the colour without lifting the level —
        // a tint that also brightened would be two controls in one.
        let push = tint - vec3<f32>(luma(tint));
        out = out + push * shadow_tone * shadow_w * 0.12;
    }
    if highlight_tone > 0.0 {
        let tint = hsv_to_rgb(vec3<f32>(fract(highlight_hue / 360.0), 1.0, 1.0));
        let push = tint - vec3<f32>(luma(tint));
        out = out + push * highlight_tone * highlight_w * 0.12;
    }

    return mix(c, out, strength);
}
