//! Kroma's palette, and the ramps drawn under its sliders.
//!
//! Every colour in the application comes from here. That is not tidiness for
//! its own sake: before this existed the greys were written as
//! `Color32::from_gray(24)` at each call site, and they had already drifted —
//! the viewer surround, the filmstrip and the status bar were three different
//! shades of what was meant to be the same background.
//!
//! It is a crate of its own, rather than a file inside the Windows shell,
//! because the Mac shell needs the same numbers and a binary crate is a place
//! nothing else can reach. Two palettes drift for exactly the reason two call
//! sites did, only further apart and with nothing compiling both.
//!
//! The scheme is Resolve's, read off the colour page. It is built from very
//! few values, which is most of why it reads as one application rather than a
//! collection of panels:
//!
//! - four greys for surfaces, from the viewer surround up to a raised header,
//! - one hairline for every division,
//! - three text weights, and
//! - a single warm accent, spent only on what is *active*.
//!
//! Resolve's own restraint is the part worth copying. Its interface is almost
//! entirely grey, so the one orange title tells you where you are without
//! having to be loud. An accent used on every heading would say nothing.
//!
//! Nothing here knows about a toolkit, and it depends on nothing. A colour is
//! an [`Rgb8`]; each shell keeps its own one-line conversion — `Color32` for
//! egui, `Color` for SwiftUI — and that conversion is all the glue there is.

mod ramp;

pub use ramp::{CHANNEL_AXES, Ramp, ramp_for};

/// A colour, as it ends up on screen.
///
/// Eight bits a channel and no alpha, because that is what every entry in the
/// palette is and what every ramp evaluates to. `repr(C)` so the same three
/// bytes can cross the ABI unchanged if a shell ever wants them that way.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct Rgb8 {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb8 {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }
}

/// Declare the palette, and the list of it, from one set of lines.
///
/// [`colour::ALL`] is what crosses to Swift. A constant written by hand beside
/// the others would compile, be used by the Windows shell, and never appear in
/// the fixture — so the Mac would quietly go on drawing something else, which
/// is the drift this crate was built to stop. Declaring and listing from the
/// same lines is the only arrangement where forgetting is not possible.
macro_rules! palette {
    ($( $(#[$doc:meta])* $name:ident = ($r:literal, $g:literal, $b:literal); )*) => {
        $( $(#[$doc])* pub const $name: Rgb8 = Rgb8::new($r, $g, $b); )*

        /// Every colour above, by name, in the order they are declared.
        ///
        /// The whole palette, for anything that has to hand the scheme to
        /// something outside Rust — the fixture the Swift tests decode is the
        /// one caller today.
        pub const ALL: &[(&str, &Rgb8)] = &[$( (stringify!($name), &$name) ),*];
    };
}

pub mod colour {
    use super::Rgb8;

    palette! {
        // ---- Surfaces, darkest to lightest -----------------------------------
        /// Behind the photograph. Darkest, so nothing in the frame competes with
        /// it — a surround lighter than the picture's own shadows makes the
        /// shadows look lifted, which is a lie told to someone grading them.
        VIEWER = (18, 18, 18);
        /// The inside of anything you type into or read a graph out of.
        WELL = (22, 22, 22);
        /// Panel background — the inspector, the scopes, the filmstrip.
        PANEL = (33, 33, 33);
        /// One step up: headers, the toolbar, a hovered row.
        RAISED = (43, 43, 43);
        /// A control that sits on a panel: buttons, combo boxes, tiles.
        CONTROL = (56, 56, 56);
        CONTROL_HOT = (70, 70, 70);

        // ---- Lines -----------------------------------------------------------
        /// Every division in the interface is this one hairline.
        RULE = (58, 58, 58);
        BOX_EDGE = (70, 70, 70);
        /// The inside of the boxed number.
        BOX_FILL = (20, 20, 20);

        // ---- Text ------------------------------------------------------------
        TITLE = (228, 228, 228);
        LABEL = (176, 176, 176);
        DIM = (128, 128, 128);
        ICON = (150, 150, 150);

        // ---- Controls --------------------------------------------------------
        TRACK = (74, 74, 74);
        /// How far the value has been pushed from neutral.
        TRACK_FILL = (122, 122, 122);
        HANDLE = (190, 190, 190);
        HANDLE_HOT = (240, 240, 240);
        /// Drawn around the handle so it stays legible on a coloured track.
        HANDLE_EDGE = (16, 16, 16);

        /// The grid inside a plot — the curve editor, the scopes, the histogram.
        GRID = (44, 44, 44);

        // ---- The channels ----------------------------------------------------
        // Red, green and blue, wherever a channel has to be named by colour: a
        // curve trace, a parade panel, a mixer band. One set, because three
        // slightly different reds across three panels reads as three different
        // meanings to anyone who has not seen the source.
        //
        // Named one at a time rather than written as an array so each reaches
        // `ALL`, and so the Mac gets them under the same names; [`CHANNEL`]
        // puts them back in the order a parade draws them.
        CHANNEL_R = (226, 86, 86);
        CHANNEL_G = (92, 206, 110);
        CHANNEL_B = (104, 142, 240);

        // ---- The accent ------------------------------------------------------
        /// Resolve titles the open effect in this, and spends it nowhere else.
        ACCENT = (224, 106, 90);
        /// The accent, dimmed for a fill behind text.
        ACCENT_DIM = (70, 26, 24);
        /// Selection. Resolve's is a muted blue, deliberately not the accent —
        /// "this is chosen" and "this is doing something" are different facts.
        SELECT = (46, 84, 122);
        WARN = (226, 168, 74);
        ERROR = (226, 96, 82);
    }

    /// The three channel colours, in channel order.
    pub const CHANNEL: [Rgb8; 3] = [CHANNEL_R, CHANNEL_G, CHANNEL_B];
}

#[cfg(test)]
mod tests {
    use super::colour;

    /// Nothing may declare a palette colour except `palette!`.
    ///
    /// The macro cannot forget to list what it declares, so the only way left
    /// to keep a colour out of [`colour::ALL`] — and therefore away from the
    /// Mac — is to declare it some other way. This is the check that the other
    /// way was not taken. Read off this file's own source, so it sees a hand
    /// written constant the macro never hears about.
    #[test]
    fn no_palette_colour_is_declared_outside_the_macro() {
        let stray: Vec<&str> = include_str!("lib.rs")
            .lines()
            .map(str::trim)
            .filter(|l| l.starts_with("pub const") && l.contains(": Rgb8") && !l.contains('$'))
            .collect();
        assert!(
            stray.is_empty(),
            "these colours are invisible to Swift — declare them inside `palette!`: {stray:?}"
        );
    }

    /// And two colours under one name would put one of them past the fixture
    /// just as effectively.
    #[test]
    fn every_palette_name_is_its_own() {
        let names: std::collections::BTreeSet<&str> = colour::ALL.iter().map(|(n, _)| *n).collect();
        assert_eq!(names.len(), colour::ALL.len(), "a palette name is repeated");
    }
}
