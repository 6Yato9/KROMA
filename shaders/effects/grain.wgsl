// Log. Slots:
//    0 preset      1 composite   2 opacity    3 grain_only
//    4 texture     5 size (microns)  6 aspect  7 strength
//    8 offset      9 symmetry   10 softness  11 saturation
//   12 red        13 green      14 blue
//   15 shadow_gain 16 midtone_gain 17 highlight_gain
//
// Slots follow the order the parameters are declared in the registry, so that
// order is load-bearing and this comment has to move with it.
//
// Grain lives in log because film grain is a density fluctuation in the
// negative, not a light phenomenon. Applied in linear it vanishes from the
// shadows, which is exactly backwards from how film behaves.
//
// Size is in microns on the negative, never pixels. That is what makes a
// 1200px preview and a 6000px export show the same grain rather than
// invisible fizz — and it is why the format preset is a real control and not
// a label: the same emulsion on a 16mm frame is magnified nearly three times
// as much by the time it reaches the same print.
//
// Parameter set follows Resolve's Film Grain, less the two temporal controls.
// Freeze and Animate On Every Refresh both describe what the grain does
// between frames, and there is no next frame here.

/// Width of the negative in microns, by format. This is the number the grain
/// size is measured against, so it is what makes 16mm look like 16mm.
fn frame_width_um(preset: f32) -> f32 {
    let i = i32(round(preset));
    switch i {
        case 0: { return 12500.0; }  // Super 16
        case 2: { return 52500.0; }  // 65mm
        default: { return 36000.0; } // 35mm still, and Custom
    }
}

/// The grain layer, composited onto the picture.
///
/// A grain plugin does not add its noise: it builds a mid-grey layer with the
/// noise in it and blends that layer over the image. Which blend is the
/// Composite Type, and it matters — Overlay leaves the black and white ends
/// alone and puts the grain in the midtones, which is where film puts it.
fn composite_grain(base: vec3<f32>, grain: vec3<f32>, mode: f32, opacity: f32) -> vec3<f32> {
    let layer = clamp(vec3<f32>(0.5) + grain, vec3<f32>(0.0), vec3<f32>(1.0));
    let i = i32(round(mode));
    var out = base;
    switch i {
        // Overlay.
        case 0: {
            out = select(
                vec3<f32>(1.0) - 2.0 * (vec3<f32>(1.0) - base) * (vec3<f32>(1.0) - layer),
                2.0 * base * layer,
                base < vec3<f32>(0.5)
            );
        }
        // Soft light, Pegtop's formula: continuous everywhere, unlike the
        // piecewise version, so a grain layer does not leave a seam at 50%.
        case 1: {
            out = (1.0 - 2.0 * layer) * base * base + 2.0 * layer * base;
        }
        // Add. The layer is mid-grey plus noise, so the grey has to come back
        // off or every frame would be lifted half a stop.
        case 2: {
            out = base + (layer - vec3<f32>(0.5));
        }
        // Screen.
        default: {
            out = vec3<f32>(1.0) - (vec3<f32>(1.0) - base) * (vec3<f32>(1.0) - layer);
        }
    }
    return mix(base, out, clamp(opacity, 0.0, 1.0));
}

fn effect(c: vec3<f32>, uv: vec2<f32>) -> vec3<f32> {
    let preset = slot(0u);
    let composite = slot(1u);
    let opacity = slot(2u);
    let grain_only = slot(3u) > 0.5;
    let texture = clamp(slot(4u), 0.0, 1.0);
    let size_um = max(slot(5u), 0.01);
    let aspect_ratio = max(slot(6u), 0.05);
    let strength = slot(7u);
    let offset = slot(8u);
    let symmetry = clamp(slot(9u), 0.0, 1.0);
    let softness = slot(10u);
    let saturation = slot(11u);
    let channels = vec3<f32>(slot(12u), slot(13u), slot(14u));
    let shadow_gain = slot(15u);
    let midtone_gain = slot(16u);
    let highlight_gain = slot(17u);

    if strength <= 0.0 && !grain_only {
        return c;
    }

    // Grains across the frame, for this format.
    let across = frame_width_um(preset) / size_um;
    let f = frame_size();
    let frame_ratio = f.y / max(f.x, 1.0);
    // Aspect ratio stretches the lattice. Real emulsion grains are not round,
    // and an anamorphic squeeze makes them less so.
    let lattice = vec2<f32>(across / aspect_ratio, across * frame_ratio * aspect_ratio);
    // Frame coordinates: grain belongs to the negative, so it must not crawl
    // across the picture when the view is panned.
    let scaled = frame_uv(uv) * lattice + vec2<f32>(u.seed);
    let cell = floor(scaled);

    var mono = hash21(cell) - 0.5;
    var n = vec3<f32>(
        mono,
        hash21(cell + vec2<f32>(17.0, 3.0)) - 0.5,
        hash21(cell + vec2<f32>(5.0, 29.0)) - 0.5,
    );

    // Texture: a second, coarser octave mixed in. Fine emulsions read as even
    // fizz and coarse ones clump, and one lattice can only do the first — the
    // clumping is what makes a fast stock look fast.
    if texture > 0.0 {
        let coarse_cell = floor(scaled * 0.4);
        let coarse = vec3<f32>(
            hash21(coarse_cell + vec2<f32>(3.0, 11.0)) - 0.5,
            hash21(coarse_cell + vec2<f32>(23.0, 7.0)) - 0.5,
            hash21(coarse_cell + vec2<f32>(13.0, 41.0)) - 0.5,
        );
        n = mix(n, n * 0.55 + coarse * 0.85, texture);
        mono = mix(mono, mono * 0.55 + (coarse.r) * 0.85, texture);
    }

    // Softness blurs the grain layer by mixing each cell toward the average of
    // its neighbours — cheaper than a real blur and enough at grain scale.
    if softness > 0.0 {
        var neighbours = 0.0;
        neighbours = neighbours + hash21(cell + vec2<f32>(1.0, 0.0));
        neighbours = neighbours + hash21(cell + vec2<f32>(-1.0, 0.0));
        neighbours = neighbours + hash21(cell + vec2<f32>(0.0, 1.0));
        neighbours = neighbours + hash21(cell + vec2<f32>(0.0, -1.0));
        let smoothed = neighbours * 0.25 - 0.5;
        n = mix(n, vec3<f32>(smoothed), clamp(softness, 0.0, 1.0));
        mono = mix(mono, smoothed, clamp(softness, 0.0, 1.0));
    }

    // Saturation 0 is monochrome grain, matching Resolve.
    n = mix(vec3<f32>(mono), n, clamp(saturation, 0.0, 2.0));

    // Symmetry: how the light and dark grains balance. A negative that has
    // been pushed has more visible dark grains than light ones, and at 0.5
    // this does nothing at all.
    if abs(symmetry - 0.5) > 1e-4 {
        let up = symmetry * 2.0;
        let down = (1.0 - symmetry) * 2.0;
        n = select(n * down, n * up, n > vec3<f32>(0.0));
    }

    // Offset lightens or darkens the whole grain layer, so lower values
    // emphasise the light grains and higher values the dark ones.
    n = n + vec3<f32>(offset * 0.25);

    // Per-channel gain. Film's three dye layers are not equally grainy — the
    // blue-sensitive layer is the worst of them — so this is not a trim, it is
    // most of what separates one stock from another.
    n = n * channels;

    // Three independent tonal gains rather than one peak position.
    //
    // Weighted against the ACEScct anchors, not 0/0.5/1. The signal here is
    // log-encoded: an SDR image spans about 0.073 to 0.555, so a highlight
    // threshold at 0.6 would never fire.
    let l = luma(c);
    let shadow_w = clamp((CCT_GREY - l) / max(CCT_GREY - CCT_BLACK, 1e-4), 0.0, 1.0);
    let highlight_w = clamp((l - CCT_GREY) / max(CCT_WHITE - CCT_GREY, 1e-4), 0.0, 1.0);
    let midtone_w = clamp(1.0 - shadow_w - highlight_w, 0.0, 1.0);
    let gain = shadow_w * shadow_gain + midtone_w * midtone_gain + highlight_w * highlight_gain;

    let layer = n * strength * 0.36 * gain;
    if grain_only {
        // The grain by itself, for judging it without the picture in the way.
        return vec3<f32>(0.5) + layer;
    }
    return composite_grain(c, layer, composite, opacity);
}
