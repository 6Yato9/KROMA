// Log. Slots 0: contrast, 1: pivot.
//
// The pivot defaults to 0.4135, which is 18% scene grey in ACEScct. Pivoting
// at 0.5 -- mid-grey in a display-referred space -- drags the whole image
// brighter as contrast rises, which reads as a broken control.
fn effect(c: vec3<f32>, uv: vec2<f32>) -> vec3<f32> {
    let amount = u.p[0].x;
    let pivot = u.p[0].y;
    // exp2 keeps the slider symmetric: -1 flattens exactly as much as +1
    // steepens, so dragging back and forth returns you where you started.
    let k = exp2(amount);
    return (c - vec3<f32>(pivot)) * k + vec3<f32>(pivot);
}
