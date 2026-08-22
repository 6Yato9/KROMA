// Log. Slots follow the registry's declaration order:
//   0 hue_divisions  1 sat_divisions
//   2 chroma_divisions  3 luma_divisions  4 axis_angle
//
// The lattices themselves are not in slots. They ride the curve LUT, two rows
// each from row 10: every vertex's displacement along the first axis, then
// along the second, row-major.
//
// Resolve's Colour Warper. Three views of one object — a grid laid over a
// two-dimensional slice of colour with a control point at each intersection —
// and the views differ only in which two axes the slice is cut along. Hue
// against saturation is one; chroma against luma, twice about two different
// axes, is the other.
//
// Log, because every axis here is perceptual. Hue, saturation and chroma are
// constructs of how a picture reads, not measurements of how much light there
// was, and a grid the user drags has to move the colour where they put it.

/// Where the lattices live in the LUT, and how wide a row is.
const WARP_ROW: i32 = 10;
const WARP_ROWS_EACH: i32 = 2;
const LUT_W: i32 = 256;

/// One vertex's displacement, from the lattice at `grid`.
fn warp_vertex(grid: i32, index: i32) -> vec2<f32> {
    let row = WARP_ROW + grid * WARP_ROWS_EACH;
    let i = clamp(index, 0, LUT_W - 1);
    return vec2<f32>(
        textureLoad(lut_texture, vec2<i32>(i, row), 0).r,
        textureLoad(lut_texture, vec2<i32>(i, row + 1), 0).r,
    );
}

/// Sample a lattice at a point in 0..1 on each axis.
///
/// `wrap` says whether the first axis is a circle. Hue is: the vertex at the
/// far right of the grid is the vertex at the far left, and treating it as an
/// edge would leave a seam at red that no amount of dragging could smooth out.
/// Chroma is not — its two ends are grey and full colour, as far apart as two
/// colours get. The second axis always clamps.
///
/// Mirrors `pe_core::Warp::sample`. If one changes, so must the other — the
/// golden suite compares the two.
fn warp_sample(grid: i32, cols: i32, rows: i32, u: f32, v: f32, wrap: bool) -> vec2<f32> {
    var c0: i32;
    var c1: i32;
    var tx: f32;
    if wrap {
        let fx = fract(u) * f32(cols);
        let x0 = i32(floor(fx));
        tx = fx - f32(x0);
        c0 = (x0 % cols + cols) % cols;
        c1 = ((x0 + 1) % cols + cols) % cols;
    } else {
        let fx = clamp(u, 0.0, 1.0) * f32(cols - 1);
        let x0 = min(i32(floor(fx)), cols - 1);
        tx = fx - f32(x0);
        c0 = x0;
        c1 = min(x0 + 1, cols - 1);
    }

    let fy = clamp(v, 0.0, 1.0) * f32(rows - 1);
    let y0 = min(i32(floor(fy)), rows - 1);
    let y1 = min(y0 + 1, rows - 1);
    let ty = fy - f32(y0);

    let top = mix(
        warp_vertex(grid, y0 * cols + c0),
        warp_vertex(grid, y0 * cols + c1),
        tx,
    );
    let bottom = mix(
        warp_vertex(grid, y1 * cols + c0),
        warp_vertex(grid, y1 * cols + c1),
        tx,
    );
    return mix(top, bottom, ty);
}

// ---------------------------------------------------------------------------
// Chroma Warp: pins on the chromaticity diagram.
// ---------------------------------------------------------------------------
//
// The other two views push hue and saturation around, which are constructs.
// This one works on *chromaticity* — where a colour sits on the CIE diagram,
// independent of how bright it is — because that is what the plot draws and
// what a pin is placed on. Moving a colour there changes its hue and its
// purity together, holds its luminance still, and is the one operation that
// means the same thing to a colourist and to a spectrophotometer.
//
// The matrices are AP1's, generated from the primaries by
// `pe-color/tests/print_matrix.rs` rather than typed in from a web page. A
// matrix transcribed by hand is a matrix with a digit wrong in it.

const PIN_ROW: i32 = 16;
const PIN_STRIDE: i32 = 12;
const MAX_PINS: i32 = 8;

const AP1_TO_XYZ: mat3x3<f32> = mat3x3<f32>(
    vec3<f32>(0.66245418, 0.27222872, -0.00557465),
    vec3<f32>(0.13400421, 0.67408177, 0.00406073),
    vec3<f32>(0.15618769, 0.05368952, 1.01033910),
);
const XYZ_TO_AP1: mat3x3<f32> = mat3x3<f32>(
    vec3<f32>(1.64102338, -0.66366286, 0.01172189),
    vec3<f32>(-0.32480329, 1.61533159, -0.00828444),
    vec3<f32>(-0.23642470, 0.01675635, 0.98839486),
);

fn pin_value(index: i32, field: i32) -> f32 {
    return textureLoad(lut_texture, vec2<i32>(index * PIN_STRIDE + field, PIN_ROW), 0).r;
}

/// How much of a pin's pull this tone takes.
///
/// Low and high are the shadow and highlight ends and the pivot is where one
/// becomes the other. Both at one is every tone equally, which is why both
/// default to one — a pin that has to be told it applies to the whole picture
/// is a pin with a control too many.
fn pin_tone(l: f32, low: f32, high: f32, pivot: f32) -> f32 {
    let t = smoothstep(pivot - 0.25, pivot + 0.25, l);
    return mix(low, high, t);
}

/// Everything the pins do to one colour.
///
/// No count is passed, and none is stored. Pins are written into the LUT from
/// index zero with no gaps, and an unused slot is all zeros — so a range of
/// zero *is* the end of the list. One texture fetch answers "are there any
/// pins at all", which is the answer on almost every frame.
fn chroma_warp(c: vec3<f32>) -> vec3<f32> {
    if pin_value(0, 4) <= 0.0 {
        return c;
    }
    let lin = max(cct_decode(c), vec3<f32>(0.0));
    let xyz = AP1_TO_XYZ * lin;
    let sum = xyz.x + xyz.y + xyz.z;
    if sum <= 1e-6 {
        // Black has no chromaticity to move.
        return c;
    }
    var xy = vec2<f32>(xyz.x / sum, xyz.y / sum);
    // Luminance is held: a chroma warp changes which colour something is, not
    // how much light it is. Exposure is the one control here that may.
    var y = max(xyz.y, 0.0);
    let l = clamp((luma(c) - CCT_BLACK) / (CCT_WHITE - CCT_BLACK), 0.0, 1.0);

    for (var i = 0; i < MAX_PINS; i = i + 1) {
        let range = pin_value(i, 4);
        if range <= 0.0 {
            break;
        }
        let at = vec2<f32>(pin_value(i, 0), pin_value(i, 1));
        let to = vec2<f32>(pin_value(i, 2), pin_value(i, 3));
        let tone = pin_tone(l, pin_value(i, 5), pin_value(i, 6), pin_value(i, 7));
        let exposure = pin_value(i, 8);

        // Measured from where the pin was *placed*, not from where it has been
        // dragged to. The pin marks a colour in the picture and then says
        // where that colour should go; measuring from the destination would
        // make the selection move as you drag it, which is a control that
        // slides out from under you.
        let d = distance(xy, at);
        let w = (1.0 - smoothstep(range * 0.5, range, d)) * tone;
        if w > 0.0 {
            xy = xy + (to - at) * w;
            y = y * pow(2.0, exposure * w);
        }
    }

    // Back to a colour: the moved chromaticity at the luminance we kept.
    let out_xyz = vec3<f32>(xy.x * y / max(xy.y, 1e-5), y, (1.0 - xy.x - xy.y) * y / max(xy.y, 1e-5));
    return cct_encode(max(XYZ_TO_AP1 * out_xyz, vec3<f32>(0.0)));
}

/// The divisions dropdowns hold an index into 4 / 6 / 8 / 12 / 16.
fn divisions(index: f32) -> i32 {
    let i = i32(round(index));
    switch i {
        case 0: { return 4; }
        case 2: { return 8; }
        case 3: { return 12; }
        case 4: { return 16; }
        default: { return 6; }
    }
}

/// How far a full drag across the grid moves the picture.
///
/// The lattice stores displacements in the units its axes are drawn in — a
/// whole turn of hue is 1.0 — so hue needs no scaling at all. Saturation and
/// chroma do: dragging a vertex the full height of the grid should be a
/// decisive move and not a destroyed picture.
const SAT_REACH: f32 = 1.0;
const LUMA_REACH: f32 = 0.35;

fn effect(c: vec3<f32>, uv: vec2<f32>) -> vec3<f32> {
    let hue_cols = divisions(slot(0u));
    let hue_rows = divisions(slot(1u));
    let chroma_cols = divisions(slot(2u));
    let chroma_rows = divisions(slot(3u));
    let axis = radians(slot(4u));

    // The pins first. They work on chromaticity, which the grids then push
    // around as hue and saturation — the other order would have the grids
    // move a colour out from under the pin that was aimed at it.
    let warped = chroma_warp(c);

    var hsv = rgb_to_hsv(max(warped, vec3<f32>(0.0)));

    // ---- Hue against saturation ------------------------------------------
    // Read at the colour's own hue and saturation, which is what makes the
    // grid feel like it is attached to the picture: the vertex you drag is
    // the one sitting on the colour you are looking at.
    let hs = warp_sample(0, hue_cols, hue_rows, hsv.x, clamp(hsv.y, 0.0, 1.0), true);
    hsv.x = fract(hsv.x + hs.x);
    hsv.y = clamp(hsv.y + hs.y * SAT_REACH, 0.0, 1.0);

    // ---- Chroma against luma ---------------------------------------------
    // Two grids about two different chromaticity axes. One alone can only push
    // colour along its own axis, which is a line through the picture's colour
    // rather than a region of it; Axis Angle is what separates them.
    //
    // The axis picks *which* chroma is being warped: a colour is weighted by
    // how closely its hue lines up with the axis, so a grid pointed at orange
    // leaves the blues alone.
    let luma_in = clamp((luma(c) - CCT_BLACK) / (CCT_WHITE - CCT_BLACK), 0.0, 1.0);
    let chroma_in = clamp(hsv.y, 0.0, 1.0);
    for (var g = 0; g < 2; g = g + 1) {
        let axis_hue = fract((axis + f32(g) * 3.14159265) / 6.2831853);
        var d = abs(hsv.x - axis_hue);
        d = min(d, 1.0 - d);
        // Half the circle either side, falling off — so the two grids together
        // cover every hue and neither has a hard edge.
        let weight = clamp(1.0 - d * 4.0, 0.0, 1.0);
        if weight > 0.0 {
            let cl = warp_sample(1 + g, chroma_cols, chroma_rows, chroma_in, luma_in, false);
            hsv.y = clamp(hsv.y + cl.x * weight, 0.0, 1.0);
            hsv.z = max(hsv.z * (1.0 + cl.y * weight * LUMA_REACH), 0.0);
        }
    }

    return hsv_to_rgb(hsv);
}
