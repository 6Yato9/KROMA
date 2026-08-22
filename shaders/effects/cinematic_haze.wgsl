// Linear. Slots follow the registry's declaration order, which is Resolve's
// panel order, so that order is load-bearing and this list moves with it:
//
//   Depth Map          0 preview   1 quality   2 invert   3 adjust_levels
//                      4 far_limit 5 near_limit 6 depth_gamma
//   Atmospheric        7 airlight  8 density   9 resolution_loss
//                     10-12 colorize
//   Light Halos       13 threshold 14 size    15 brightness 16 saturation
//                     17-19 colorize
//   Light Rays        20 enable    21 preview  22 source_threshold
//                     23 directions 24 angle  25 length  26 soften
//                     27 brightness 28 saturation
//   Air Disturbance   29 enable    30 preview  31 intensity 32 brightness
//                     33 scale     34 detail   35 start_frame
//
// Resolve's AI Cinematic Haze, minus the AI. Theirs estimates depth with a
// network; this estimates it from the picture, and the name says so — putting
// "AI" on a dark-channel prior would be a claim made to our own user.
//
// The estimate is the dark-channel prior, which is the same observation
// Dehaze already runs backwards: in a clear photograph almost every small
// patch has *some* channel that is nearly black — a shadow, a dark surface,
// a saturated colour. Haze is bright and grey, so it lifts that darkest
// channel, and how far it has been lifted is how much air is in front of it.
// The prior fails on genuinely bright neutral subjects — snow, white walls,
// overcast sky — which read as distant, and that is a real limitation of the
// method rather than a bug in this file.
//
// Linear, because every part of it is light: scattering, glow, and an average
// over a disc.

/// Patch size for the dark channel, as a fraction of the frame.
///
/// It has to be a patch and not a pixel: the prior is a statement about small
/// *regions* ("something in here is dark"), and a single pixel in a bright
/// area is not evidence of haze.
const DARK_PATCH: f32 = 0.012;

fn dark_samples(quality: f32) -> i32 {
    let i = i32(round(quality));
    switch i {
        case 0: { return 6; }   // Faster
        case 2: { return 20; }  // Best
        default: { return 12; } // Better
    }
}

/// How much air is between the camera and what it is looking at, 0 to 1.
fn haze_depth(uv: vec2<f32>, quality: f32) -> f32 {
    let aspect = frame_aspect();
    let r = frame_to_uv(DARK_PATCH);
    let count = dark_samples(quality);

    var darkest = 1e9;
    for (var i = 0; i < count; i = i + 1) {
        let fi = f32(i);
        let angle = fi * 2.39996323;
        let rad = sqrt((fi + 0.5) / f32(count)) * r;
        let offset = vec2<f32>(cos(angle) * rad / max(aspect, 1e-4), sin(angle) * rad);
        let s = textureSampleLevel(src_texture, src_sampler, uv + offset, 0.0).rgb;
        darkest = min(darkest, min(s.r, min(s.g, s.b)));
    }
    // Diffuse white is 1.0 in this space, and a dark channel much above about
    // a third is thick air by any reading.
    return clamp(darkest * 3.0, 0.0, 1.0);
}

/// A value-noise field, for the heat shimmer.
fn haze_noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = hash21(i);
    let b = hash21(i + vec2<f32>(1.0, 0.0));
    let c = hash21(i + vec2<f32>(0.0, 1.0));
    let d = hash21(i + vec2<f32>(1.0, 1.0));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

/// Several octaves of it. Detail is how many, so the control is a real one
/// rather than a frequency multiplier wearing the name.
fn haze_turbulence(p: vec2<f32>, detail: f32) -> f32 {
    var sum = 0.0;
    var amp = 0.5;
    var freq = 1.0;
    var total = 0.0;
    let octaves = i32(clamp(detail, 1.0, 16.0));
    for (var i = 0; i < octaves; i = i + 1) {
        sum = sum + haze_noise(p * freq) * amp;
        total = total + amp;
        freq = freq * 2.0;
        amp = amp * 0.55;
    }
    return sum / max(total, 1e-4);
}

/// Light streaming from the bright parts of the picture.
///
/// Marched rather than gathered: a ray is a sum *along a line*, and the disc
/// samples every other glow in this application uses cannot make one — they
/// would give a round bloom whichever direction they were pointed in.
const RAY_STEPS: i32 = 24;

fn light_rays(
    uv: vec2<f32>,
    dir: vec2<f32>,
    radial: bool,
    length_t: f32,
    threshold: f32,
) -> vec3<f32> {
    let aspect = frame_aspect();
    let here = frame_uv(uv);
    // Radial rays point away from the middle, so every source throws its own
    // shafts outward; angled rays are parallel, which is sunlight.
    var step_dir = dir;
    if radial {
        let away = here - vec2<f32>(0.5);
        let len = max(length(away), 1e-4);
        step_dir = away / len;
    }
    let reach = frame_to_uv(length_t * 0.35);
    let step = vec2<f32>(step_dir.x / max(aspect, 1e-4), step_dir.y) * (reach / f32(RAY_STEPS));

    var sum = vec3<f32>(0.0);
    var total = 0.0;
    for (var i = 1; i <= RAY_STEPS; i = i + 1) {
        let t = f32(i) / f32(RAY_STEPS);
        let s = textureSampleLevel(src_texture, src_sampler, uv - step * f32(i), 0.0).rgb;
        let over = max(s - vec3<f32>(threshold), vec3<f32>(0.0));
        // Falls off along the shaft, so a ray thins out rather than stopping.
        let w = 1.0 - t;
        sum = sum + over * w;
        total = total + w;
    }
    return sum / max(total, 1e-4);
}

fn effect(c: vec3<f32>, uv: vec2<f32>) -> vec3<f32> {
    let depth_preview = slot(0u) > 0.5;
    let quality = slot(1u);
    let invert = slot(2u) > 0.5;
    let adjust_levels = slot(3u) > 0.5;
    let airlight = slot(7u);
    let density = slot(8u);
    let resolution_loss = slot(9u);
    let scatter_colour = slot3(10u);

    // ---- The depth map ---------------------------------------------------
    var depth = haze_depth(uv, quality);
    if invert {
        depth = 1.0 - depth;
    }
    if adjust_levels {
        let far = slot(4u);
        let near = slot(5u);
        let span = near - far;
        // A near limit below the far one flips the map rather than dividing
        // by something near zero, which is at least a defined thing to do.
        if abs(span) < 1e-4 {
            depth = 0.5;
        } else {
            depth = clamp((depth - far) / span, 0.0, 1.0);
        }
        depth = pow(depth, max(slot(6u), 1e-3));
    }
    if depth_preview {
        return vec3<f32>(depth);
    }

    // Everything downstream reads *distance*, and after Invert the map is
    // nearness. One flip here rather than a `1.0 -` at each use.
    let distance = 1.0 - depth;

    var out = c;

    // ---- Resolution loss -------------------------------------------------
    // Distance costs detail as well as contrast. Without this the far
    // hillside is a flat wash at full sharpness, which reads as a filter over
    // the picture rather than as air in front of it.
    if resolution_loss > 0.0 && distance > 0.0 {
        let radius = frame_to_uv(resolution_loss * distance * 0.006);
        if radius > 0.0 {
            out = mix(out, film_halo_blur(uv, radius, 1.0), clamp(distance, 0.0, 1.0));
        }
    }

    // ---- Air disturbance -------------------------------------------------
    // Before the scattering, because shimmer is the air bending light on its
    // way here — it happens to the picture, not to the haze laid over it.
    if slot(29u) > 0.5 && slot(31u) > 0.0 {
        let intensity = slot(31u);
        let brightness = slot(32u);
        let scale = max(slot(33u), 0.05);
        let detail = slot(34u);
        // Start Frame is a seed here, not a time. For one exposure it says
        // which slice of the field you got, which is the only part of an
        // animated turbulence a photograph can have.
        let seed = slot(35u) * 0.137;

        // Larger Scale means larger features, so it divides the frequency.
        let p = frame_uv(uv) * (24.0 / scale) + vec2<f32>(seed, seed * 1.31);
        let n = haze_turbulence(p, detail) - 0.5;
        let m = haze_turbulence(p + vec2<f32>(7.3, 2.1), detail) - 0.5;

        if slot(30u) > 0.5 {
            // Preview Influence: the field itself, so Scale and Detail can be
            // set by looking at them rather than inferred through the picture.
            return vec3<f32>(n + 0.5);
        }

        // Weighted by distance: shimmer accumulates along the path, so the far
        // end of a street boils and the near end does not.
        let shift = vec2<f32>(n, m) * intensity * 0.02 * distance;
        out = textureSampleLevel(src_texture, src_sampler, uv + shift, 0.0).rgb;
        // Bending light also concentrates and thins it, so the field shows as
        // brightness as well as displacement. Brightness says how much of it
        // arrives that way.
        out = out * (1.0 + n * intensity * brightness * 0.7 * distance);
    }

    // ---- Atmospheric scattering ------------------------------------------
    // The standard model: what reaches the lens is the subject attenuated by
    // the air, plus the air's own scattered light. Read backwards, this is
    // exactly what Dehaze undoes — which is why the two share a file's worth
    // of reasoning and no code.
    if density > 0.0 {
        let transmission = exp(-density * distance * 4.0);
        let air = scatter_colour * airlight;
        out = out * transmission + air * (1.0 - transmission);
    }

    // ---- Light halos -----------------------------------------------------
    // A bright thing seen through air acquires a halo, and a *distant* bright
    // thing acquires a bigger one — which is why this lives with the depth map
    // rather than being Bloom a second time.
    let halo_brightness = slot(15u);
    if halo_brightness > 0.0 && slot(14u) > 0.0 {
        let radius = slot(14u) * 0.03 * (0.35 + distance);
        var glow = film_bloom(uv, radius, slot(13u));
        glow = mix(vec3<f32>(dot(glow, AP1_LUMA)), glow, clamp(slot(16u), 0.0, 2.0));
        out = out + glow * slot3(17u) * halo_brightness;
    }

    // ---- Light rays ------------------------------------------------------
    if slot(20u) > 0.5 && slot(27u) > 0.0 {
        let threshold = slot(22u);
        if slot(21u) > 0.5 {
            // Preview Threshold: what is going to throw rays, on its own.
            return vec3<f32>(step(threshold, luma(c)));
        }
        let a = radians(slot(24u));
        let dir = vec2<f32>(cos(a), sin(a));
        let radial = i32(round(slot(23u))) == 1;
        var rays = light_rays(uv, dir, radial, slot(25u), threshold);
        // Soften spreads the shaft sideways, which is what stops it reading
        // as a smear of the source rather than as light in air.
        if slot(26u) > 0.0 {
            let soft = film_halo_blur(uv, frame_to_uv(slot(26u) * 0.01), 1.0);
            rays = mix(rays, max(soft - vec3<f32>(threshold), vec3<f32>(0.0)), slot(26u) * 0.5);
        }
        rays = mix(vec3<f32>(dot(rays, AP1_LUMA)), rays, clamp(slot(28u), 0.0, 2.0));
        out = out + rays * slot(27u);
    }

    return out;
}
