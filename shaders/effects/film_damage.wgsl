// Linear. Slots (flat indices, read with slot()/slot3()):
//    0 film_blur      1 temp_shift     2 tint_shift
//    3 focal_factor   4 geometry_factor 5 tilt_amount   6 tilt_angle
//    7 dirt_density   8 dirt_size      9 dirt_blur     10 dirt_seed
//   11 dirt_colour (3)
//   14 + 8n for scratch n: enable, colour (3), position, width, strength, blur
//
// Slots follow the order the parameters are declared in the registry, so that
// order is load-bearing and this comment has to move with it.
//
// Everything here is physical — a projector bulb running warm, dye layers
// failing, dirt occluding light, emulsion gouged away — so it runs in linear.
//
// Five independently placed scratches rather than one control with a count,
// matching Resolve. A count would not let you position them, and where a
// scratch sits is most of what makes the damage look real rather than
// procedural.
//
// Resolve's temporal controls (Changing Dirt, Moving Scratch, Flickering
// Speed) are deliberately absent: they describe how damage moves between
// frames, and there is no next frame here.

const DAMAGE_BLUR_SAMPLES: i32 = 12;

// One scratch: a soft-edged vertical band at `position`.
fn scratch_mask(uv: vec2<f32>, position: f32, width: f32, blur: f32) -> f32 {
    if width <= 0.0 {
        return 0.0;
    }
    let half_width = width * 0.5;
    let d = abs(uv.x - position);
    // Blur widens the falloff without widening the core, so a defocused
    // scratch stays in the same place.
    let feather = max(blur * 0.02, 1e-4);
    return 1.0 - smoothstep(half_width, half_width + feather, d);
}

fn effect(c: vec3<f32>, uv: vec2<f32>) -> vec3<f32> {
    let film_blur = slot(0u);
    let temp_shift = slot(1u);
    let tint_shift = slot(2u);
    let focal_factor = slot(3u);
    let geometry_factor = slot(4u);
    let tilt_amount = slot(5u);
    let tilt_angle = slot(6u);
    let dirt_density = slot(7u);
    let dirt_size = slot(8u);
    let dirt_blur = slot(9u);
    let dirt_seed = slot(10u);
    let dirt_colour = slot3(11u);

    let aspect = frame_aspect();
    // Damage belongs to the film, not to the viewport, so every position below
    // is in frame coordinates and nothing moves when the view is panned.
    let fuv = frame_uv(uv);
    var out = c;

    // --- Film blur: knock the digital sharpness off ------------------------
    if film_blur > 0.0 {
        var sum = vec3<f32>(0.0);
        var total = 0.0;
        let radius = frame_to_uv(film_blur * 0.006);
        for (var i = 0; i < DAMAGE_BLUR_SAMPLES; i = i + 1) {
            let fi = f32(i);
            let angle = fi * 2.39996323;
            let r = sqrt((fi + 0.5) / f32(DAMAGE_BLUR_SAMPLES)) * radius;
            let offset = vec2<f32>(cos(angle) * r / aspect, sin(angle) * r);
            sum = sum + textureSampleLevel(src_texture, src_sampler, uv + offset, 0.0).rgb;
            total = total + 1.0;
        }
        out = sum / max(total, 1.0);
    }

    // --- Temp and tint shift ----------------------------------------------
    // Positive temp warms, simulating the warmer bulb of a film projector.
    // Positive tint yellows, simulating dye failure. Both are channel gains
    // normalised so the shift changes colour without changing exposure.
    if temp_shift != 0.0 || tint_shift != 0.0 {
        var gains = vec3<f32>(
            1.0 + temp_shift * 0.25,
            1.0 + tint_shift * 0.12,
            1.0 - temp_shift * 0.25 - tint_shift * 0.12,
        );
        gains = gains / max(dot(gains, AP1_LUMA), 1e-4);
        out = out * gains;
    }

    // --- Lens vignetting ---------------------------------------------------
    if focal_factor > 0.0 {
        var d = fuv - vec2<f32>(0.5);
        // Tilt slides the darkening off-centre, so the top and bottom (or left
        // and right) are unevenly shaded the way a misaligned gate would.
        let tilt = radians(tilt_angle);
        d = d + vec2<f32>(cos(tilt), sin(tilt)) * tilt_amount * 0.25;
        d.x = d.x * aspect;
        let r = length(d) / 0.70710678;
        // Focal Factor sets how far in it reaches; Geometry Factor how hard
        // the edge is and how dark it gets.
        let inner = clamp(1.0 - focal_factor, 0.0, 0.999);
        let falloff = smoothstep(inner, 1.0, r);
        out = out * max(1.0 - falloff * geometry_factor, 0.0);
    }

    // --- Dirt --------------------------------------------------------------
    // Larger specks adhering to the film. Black reads as dirt on a print,
    // white as dirt on a negative, which is why the colour is a control.
    if dirt_density > 0.0 && dirt_size > 0.0 {
        let cells = 180.0 / max(dirt_size, 0.05);
        let grid = fuv * vec2<f32>(cells, cells / max(aspect, 1e-4));
        let cell = floor(grid);
        let present = hash21(cell + vec2<f32>(dirt_seed));
        if present < dirt_density {
            // Jitter within the cell so the specks are not on a lattice.
            let jitter = vec2<f32>(
                hash21(cell + vec2<f32>(7.0, dirt_seed)),
                hash21(cell + vec2<f32>(dirt_seed, 13.0)),
            );
            let d = length(fract(grid) - jitter);
            // Each speck varies in size, so they do not read as one stamp.
            let speck = 0.18 + 0.22 * hash21(cell + vec2<f32>(31.0, 3.0));
            let feather = max(dirt_blur * speck, 1e-4);
            let mask = 1.0 - smoothstep(speck, speck + feather, d);
            out = mix(out, dirt_colour, mask);
        }
    }

    // --- Scratches ---------------------------------------------------------
    // Composited one at a time rather than accumulated into a single mask,
    // because each carries its own colour now. Taking the strongest and then
    // picking one colour for all of them would make a white scratch crossing a
    // black one come out whichever was written last.
    for (var i = 0u; i < 5u; i = i + 1u) {
        let base = 14u + i * 8u;
        if slot(base) < 0.5 {
            continue;
        }
        let colour = slot3(base + 1u);
        let m = scratch_mask(fuv, slot(base + 4u), slot(base + 5u), slot(base + 7u));
        let amount = clamp(m * slot(base + 6u), 0.0, 1.0);
        if amount > 0.0 {
            out = mix(out, colour, amount);
        }
    }

    return out;
}
