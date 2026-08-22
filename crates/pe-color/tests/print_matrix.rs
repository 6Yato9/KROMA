//! Prints the AP1 <-> XYZ matrices so the Colour Warper's shader can carry
//! generated copies rather than transcribed ones.
//!
//! Not a test of behaviour. It exists because a matrix typed by hand into WGSL
//! is a matrix with a digit wrong in it, and because the numbers should be
//! traceable to the primaries they came from rather than to a web page.
//!
//! Run with `cargo test -p pe-color --test print_matrix -- --nocapture`.

#[test]
fn print_ap1_matrices() {
    let fwd = pe_color::primaries::AP1.rgb_to_xyz();
    let inv = fwd.inverse().expect("AP1 is invertible");
    println!("AP1->XYZ");
    for row in fwd.0 {
        println!(
            "    vec3<f32>({:.8}, {:.8}, {:.8}),",
            row[0], row[1], row[2]
        );
    }
    println!("XYZ->AP1");
    for row in inv.0 {
        println!(
            "    vec3<f32>({:.8}, {:.8}, {:.8}),",
            row[0], row[1], row[2]
        );
    }
}
