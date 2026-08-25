//! What a slider's track is filled with, and which parameter gets which.

use crate::{Rgb8, colour};

/// What a slider's track is filled with.
///
/// A grey track says nothing about what it does; a track that runs blue to
/// yellow says *temperature* before the label is read. The rule for adding
/// one: the gradient must show the parameter's own axis. Exposure gets no
/// ramp, because a black-to-white track under a control that moves the whole
/// picture would be decoration pretending to be information.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub enum Ramp {
    #[default]
    Plain,
    /// Cool to warm, through neutral.
    Temp,
    /// Green to magenta, through neutral — the other white-balance axis.
    Tint,
    /// The whole hue circle.
    Hue,
    /// A window of the hue circle, centred on one band's own colour, so the
    /// mixer's Red row shows what red actually shifts towards.
    HueAround(f32),
    /// Grey to that band's colour.
    Sat(Rgb8),
    /// Grey to increasingly colourful, across the spectrum. What a master
    /// saturation control does, drawn.
    Chroma,
    /// Black to white.
    Luma,
    /// One channel's own axis, through neutral: cyan to red, magenta to
    /// green, yellow to blue. What a wheel's Red slider actually does is
    /// take red *out* on the way down, and taking red out is adding cyan.
    Axis(Rgb8, Rgb8),
}

impl Ramp {
    /// The colour at `t` along the track.
    pub fn at(self, t: f32) -> Rgb8 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Self::Plain => colour::TRACK,
            Self::Temp => mix3(
                Rgb8::new(48, 108, 214),
                Rgb8::new(126, 126, 126),
                Rgb8::new(232, 196, 88),
                t,
            ),
            Self::Tint => mix3(
                Rgb8::new(62, 190, 104),
                Rgb8::new(126, 126, 126),
                Rgb8::new(204, 84, 196),
                t,
            ),
            Self::Hue => hsv(t, 0.80, 0.86),
            // A little over a fifth of the circle either side: wide enough to
            // show which way the neighbours lie, narrow enough that the ends
            // are not some other colour entirely.
            Self::HueAround(deg) => hsv((deg / 360.0) + (t - 0.5) * 0.22, 0.80, 0.86),
            Self::Sat(vivid) => lerp(Rgb8::new(104, 104, 104), vivid, t),
            // Grey at the left, vivid at the right, at a roughly constant
            // lightness — a saturation control does not change how bright the
            // picture is, and a ramp that got brighter as it got more colourful
            // would say it does.
            Self::Chroma => hsv(t, t * 0.9, 0.46 + t * 0.4),
            Self::Luma => lerp(Rgb8::new(14, 14, 14), Rgb8::new(236, 236, 236), t),
            Self::Axis(neg, pos) => mix3(neg, Rgb8::new(126, 126, 126), pos, t),
        }
    }

    pub fn is_plain(self) -> bool {
        self == Self::Plain
    }

    /// How a ramp is spelled where something outside Rust has to read it.
    ///
    /// Deliberately not `{:?}`. The derived `Debug` spells a saturation ramp
    /// `Sat(Rgb8 { r: 35, g: 228, b: 235 })`, and that is a formatting
    /// accident rather than a contract: it names [`Rgb8`]'s field layout, so
    /// adding a field there would silently rewrite a string the Swift suite is
    /// asserted against. Written out here, changing it is a decision someone
    /// takes on purpose and both sides feel.
    ///
    /// Spelled in lower camel case because the mirror of this lives in Swift,
    /// where that is what a case is called.
    pub fn tag(&self) -> String {
        match *self {
            Self::Plain => "plain".to_string(),
            Self::Temp => "temp".to_string(),
            Self::Tint => "tint".to_string(),
            Self::Hue => "hue".to_string(),
            Self::HueAround(deg) => format!("hueAround({})", degrees(deg)),
            Self::Sat(vivid) => format!("sat({})", bytes(vivid)),
            Self::Chroma => "chroma".to_string(),
            Self::Luma => "luma".to_string(),
            Self::Axis(neg, pos) => format!("axis({},{})", bytes(neg), bytes(pos)),
        }
    }
}

fn bytes(c: Rgb8) -> String {
    format!("{},{},{}", c.r, c.g, c.b)
}

/// A hue, in degrees, spelled the way it was written down.
///
/// Every band's hue is a whole number of degrees, so `28` rather than `28.0`
/// or `2.8e1` — the two things Rust and Swift would otherwise each choose for
/// themselves. Anything that is not whole gets three places, which is finer
/// than the hue circle can show and is the same string on both sides.
fn degrees(deg: f32) -> String {
    if deg.is_finite() && deg.fract() == 0.0 && deg.abs() < 1e9 {
        format!("{}", deg as i64)
    } else {
        format!("{deg:.3}")
    }
}

/// HSV to a display colour.
///
/// Done here rather than through `egui::ecolor::Hsva`, whose components are
/// *linear* — `Hsva::new(0.0, 0.85, 0.92)` converts to a display red of 246,
/// not 235, and every ramp built that way came out looking bleached beside
/// the hand-picked colours next to it. These numbers are the ones that end up
/// on screen, which is what a palette wants.
fn hsv(h: f32, s: f32, v: f32) -> Rgb8 {
    let h = h.rem_euclid(1.0) * 6.0;
    let c = v * s;
    let x = c * (1.0 - (h % 2.0 - 1.0).abs());
    let (r, g, b) = match h as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = v - c;
    let byte = |f: f32| ((f + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    Rgb8::new(byte(r), byte(g), byte(b))
}

fn lerp(a: Rgb8, b: Rgb8, t: f32) -> Rgb8 {
    let f = |x: u8, y: u8| {
        (x as f32 + (y as f32 - x as f32) * t)
            .round()
            .clamp(0.0, 255.0) as u8
    };
    Rgb8::new(f(a.r, b.r), f(a.g, b.g), f(a.b, b.b))
}

fn mix3(a: Rgb8, mid: Rgb8, b: Rgb8, t: f32) -> Rgb8 {
    if t < 0.5 {
        lerp(a, mid, t * 2.0)
    } else {
        lerp(mid, b, (t - 0.5) * 2.0)
    }
}

/// The three channel axes, in the order a wheel's sliders are drawn.
pub const CHANNEL_AXES: [Ramp; 3] = [
    Ramp::Axis(Rgb8::new(72, 200, 208), Rgb8::new(226, 78, 72)),
    Ramp::Axis(Rgb8::new(210, 76, 190), Rgb8::new(94, 202, 96)),
    Ramp::Axis(Rgb8::new(226, 206, 84), Rgb8::new(86, 122, 226)),
];

/// The colour a mixer band is named after.
fn band_hue(name: &str) -> Option<f32> {
    Some(match name {
        "red" => 0.0,
        "orange" => 28.0,
        "yellow" => 52.0,
        "green" => 110.0,
        "aqua" | "cyan" => 182.0,
        "blue" => 222.0,
        "purple" => 272.0,
        "magenta" => 312.0,
        _ => return None,
    })
}

/// Which ramp a parameter gets, decided from its key.
///
/// Keyed off the parameter rather than listed per panel because the same
/// parameter appears in several: Temp. Shift inside Film Damage is the same
/// axis as Temperature in Basic, and a user who has learnt that blue-to-yellow
/// means white balance should not have to learn it twice.
///
/// The matches are on whole words, not substrings. `contains("tint")` looked
/// fine until a `tilt` or a `saturation` inside some unrelated effect picked
/// up a gradient that made a promise the control does not keep.
pub fn ramp_for(effect: &str, key: &str) -> Ramp {
    // The colour mixer's three rows per band.
    if let Some((band, axis)) = key.split_once('_')
        && let Some(deg) = band_hue(band)
    {
        return match axis {
            "hue" => Ramp::HueAround(deg),
            "saturation" | "sat" => Ramp::Sat(hsv(deg / 360.0, 0.85, 0.92)),
            "luminance" | "lum" => Ramp::Luma,
            _ => Ramp::Plain,
        };
    }
    match (effect, key) {
        (_, "temperature" | "temp" | "temp_shift") => Ramp::Temp,
        (_, "tint" | "tint_shift") => Ramp::Tint,
        (_, "hue" | "hue_rotate") => Ramp::Hue,
        (_, "saturation" | "vibrance" | "sat") => Ramp::Chroma,
        _ => Ramp::Plain,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A ramp has to actually ramp, or it is a plain track drawn the
    /// expensive way.
    ///
    /// Counted across the track rather than by comparing the two ends,
    /// because one ramp legitimately has the same colour at both: the hue
    /// circle closes, and a Hue track that did *not* come back to red would
    /// be the broken one.
    #[test]
    fn every_ramp_actually_ramps() {
        for ramp in [
            Ramp::Temp,
            Ramp::Tint,
            Ramp::Hue,
            Ramp::HueAround(0.0),
            Ramp::Sat(Rgb8::new(255, 0, 0)),
            Ramp::Chroma,
            Ramp::Luma,
            Ramp::Axis(Rgb8::new(0, 0, 255), Rgb8::new(255, 0, 0)),
        ] {
            let seen: std::collections::BTreeSet<[u8; 3]> = (0..=8)
                .map(|i| ramp.at(i as f32 / 8.0))
                .map(|c| [c.r, c.g, c.b])
                .collect();
            assert!(
                seen.len() >= 4,
                "{ramp:?} only reaches {} colours across its track",
                seen.len()
            );
        }
    }

    /// And the hue circle in particular has to close.
    #[test]
    fn the_hue_ramp_comes_back_round() {
        assert_eq!(Ramp::Hue.at(0.0), Ramp::Hue.at(1.0));
        assert_ne!(Ramp::Hue.at(0.0), Ramp::Hue.at(0.5));
    }

    /// Both white-balance axes have to pass through neutral in the middle.
    /// A temperature slider whose centre is tinted would put the *look* of a
    /// cast on a value that has none.
    #[test]
    fn the_white_balance_ramps_are_neutral_in_the_middle() {
        for ramp in [Ramp::Temp, Ramp::Tint] {
            let mid = ramp.at(0.5);
            assert!(
                mid.r.abs_diff(mid.g) < 6 && mid.g.abs_diff(mid.b) < 6,
                "the centre of the ramp is tinted: {mid:?}"
            );
        }
    }

    #[test]
    fn the_mixer_bands_each_get_their_own_colour() {
        assert!(matches!(
            ramp_for("colour_mixer", "red_hue"),
            Ramp::HueAround(h) if h == 0.0
        ));
        assert!(matches!(
            ramp_for("colour_mixer", "blue_hue"),
            Ramp::HueAround(h) if h == 222.0
        ));
        assert!(matches!(
            ramp_for("colour_mixer", "green_saturation"),
            Ramp::Sat(_)
        ));
        assert!(matches!(
            ramp_for("colour_mixer", "aqua_luminance"),
            Ramp::Luma
        ));
    }

    /// The same axis, wherever it turns up.
    #[test]
    fn temperature_is_the_same_ramp_in_every_effect() {
        assert_eq!(
            ramp_for("white_balance", "temperature"),
            ramp_for("film_damage", "temp_shift")
        );
        assert_eq!(
            ramp_for("white_balance", "tint"),
            ramp_for("film_damage", "tint_shift")
        );
    }

    /// The reason the matches are on whole words. Every one of these read as
    /// a white-balance or hue control under a `contains` test, and each would
    /// have put a gradient on a slider that does something else entirely.
    #[test]
    fn parameters_that_merely_look_like_colour_controls_stay_plain() {
        for (effect, key) in [
            ("film_damage", "tilt_amount"),
            ("film_damage", "tilt_angle"),
            ("tone", "highlights"),
            ("grain", "shadow_gain"),
            ("contrast", "contrast"),
            ("presence", "texture"),
            ("dehaze", "display_depth"),
        ] {
            assert!(
                ramp_for(effect, key).is_plain(),
                "{effect}.{key} was given a gradient it has no business with"
            );
        }
    }

    /// The ramp table is matched on whole words. `contains("tint")` looked fine
    /// until a `tilt` or a `saturation` inside some unrelated effect picked up a
    /// gradient that made a promise the control does not keep.
    #[test]
    fn a_ramp_is_matched_on_the_whole_key() {
        assert_eq!(ramp_for("white_balance", "temperature"), Ramp::Temp);
        assert_eq!(ramp_for("anything", "tint"), Ramp::Tint);
        assert!(ramp_for("anything", "tilt").is_plain());
        assert!(ramp_for("anything", "desaturation_amount").is_plain());
    }

    #[test]
    fn a_mixer_band_gets_its_own_hue_window() {
        assert_eq!(ramp_for("colour_mixer", "red_hue"), Ramp::HueAround(0.0));
        assert!(matches!(
            ramp_for("colour_mixer", "green_saturation"),
            Ramp::Sat(_)
        ));
        assert_eq!(ramp_for("colour_mixer", "blue_luminance"), Ramp::Luma);
        // A band nobody named is not a band.
        assert!(ramp_for("colour_mixer", "beige_hue").is_plain());
    }

    /// A saturation ramp does not get brighter as it gets more colourful — a
    /// saturation control does not change how bright the picture is, and a ramp
    /// that said it did would be lying about the parameter.
    #[test]
    fn the_chroma_ramp_holds_its_lightness() {
        let ends = [Ramp::Chroma.at(0.15), Ramp::Chroma.at(0.85)];
        let luma = |c: Rgb8| 0.2126 * c.r as f32 + 0.7152 * c.g as f32 + 0.0722 * c.b as f32;
        assert!(
            (luma(ends[0]) - luma(ends[1])).abs() < 60.0,
            "the chroma ramp changes lightness across its span: {:?}",
            ends
        );
    }

    /// The spelling the fixture carries, written out.
    ///
    /// Asserted literally, because the whole point of `tag` is that it does
    /// not come from anywhere that could change on its own. A test that built
    /// the expected string from the same code would agree with any spelling at
    /// all, including the derived one this replaced.
    #[test]
    fn a_ramp_is_spelled_the_same_way_on_both_sides() {
        assert_eq!(Ramp::Plain.tag(), "plain");
        assert_eq!(Ramp::Temp.tag(), "temp");
        assert_eq!(Ramp::Tint.tag(), "tint");
        assert_eq!(Ramp::Hue.tag(), "hue");
        assert_eq!(Ramp::Chroma.tag(), "chroma");
        assert_eq!(Ramp::Luma.tag(), "luma");
        assert_eq!(Ramp::HueAround(28.0).tag(), "hueAround(28)");
        assert_eq!(Ramp::HueAround(0.0).tag(), "hueAround(0)");
        assert_eq!(Ramp::HueAround(182.5).tag(), "hueAround(182.500)");
        assert_eq!(Ramp::Sat(Rgb8::new(35, 228, 235)).tag(), "sat(35,228,235)");
        assert_eq!(
            Ramp::Axis(Rgb8::new(72, 200, 208), Rgb8::new(226, 78, 72)).tag(),
            "axis(72,200,208,226,78,72)"
        );
    }

    /// And no two ramps share a spelling, or the Swift side would draw one of
    /// them wherever the fixture named the other.
    #[test]
    fn no_two_ramps_are_spelled_alike() {
        let all = [
            Ramp::Plain,
            Ramp::Temp,
            Ramp::Tint,
            Ramp::Hue,
            Ramp::HueAround(28.0),
            Ramp::HueAround(52.0),
            Ramp::Sat(Rgb8::new(235, 35, 35)),
            Ramp::Sat(Rgb8::new(35, 235, 35)),
            Ramp::Chroma,
            Ramp::Luma,
            Ramp::Axis(Rgb8::new(72, 200, 208), Rgb8::new(226, 78, 72)),
        ];
        let tags: std::collections::BTreeSet<String> = all.iter().map(Ramp::tag).collect();
        assert_eq!(tags.len(), all.len(), "two ramps spell the same: {tags:?}");
    }

    #[test]
    fn every_ramp_is_defined_across_its_whole_span_and_clamps_outside() {
        for ramp in [
            Ramp::Plain,
            Ramp::Temp,
            Ramp::Tint,
            Ramp::Hue,
            Ramp::HueAround(120.0),
            Ramp::Sat(Rgb8::new(200, 80, 80)),
            Ramp::Chroma,
            Ramp::Luma,
            Ramp::Axis(Rgb8::new(0, 200, 200), Rgb8::new(200, 0, 0)),
        ] {
            for t in [-1.0_f32, 0.0, 0.5, 1.0, 2.0] {
                let _ = ramp.at(t);
            }
            assert_eq!(ramp.at(-1.0), ramp.at(0.0), "{ramp:?} does not clamp low");
            assert_eq!(ramp.at(2.0), ramp.at(1.0), "{ramp:?} does not clamp high");
        }
    }
}
