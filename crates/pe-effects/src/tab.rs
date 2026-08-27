//! The inspector's four tabs, and the five sections of the first of them.
//!
//! `apps/windows`'s shape, and now this shell's: a row of four tabs — Colour,
//! Effects, Image, File — with the whole grade under the first, divided into
//! collapsing sections. Resolve's colour page is the same arrangement, and it
//! exists to avoid the thing the alternative does: eleven pinned panels in one
//! scrolling column, where reaching the warper means scrolling past a hundred
//! and thirty controls and every title on the way wears the accent at once.
//!
//! The Mac shell drew eight icons for a while, which was the five sections of
//! the Colour tab promoted to peers of the other three tabs. That is why this
//! file names two levels rather than one.
//!
//! Shared rather than written per shell because it is one answer per effect:
//! which section draws a pinned row is not a thing two shells should be able to
//! disagree about.

/// One of the inspector's four tabs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tab {
    /// The grade: five collapsing sections, listed by [`Section::ALL`].
    #[default]
    Colour,
    /// The stack the user built, and the shelf that adds to it.
    Effects,
    /// The crop, the straightening angle, the quarter-turns and the flips.
    ///
    /// The one tab that edits the document's *geometry* rather than a row in
    /// its stack, which is why no section belongs to it: there is no `Effect`
    /// behind it and no parameters to look up.
    Image,
    /// What the photograph is, and what it will be written as.
    File,
}

impl Tab {
    /// Every tab, in the order the row shows them.
    ///
    /// Here rather than in a shell for the same reason [`Section::ALL`] is: a
    /// variant added and not listed is a tab with no button, and the only
    /// symptom is a page nobody can reach.
    pub const ALL: [Tab; 4] = [Tab::Colour, Tab::Effects, Tab::Image, Tab::File];

    pub fn name(self) -> &'static str {
        match self {
            Tab::Colour => "Colour",
            Tab::Effects => "Effects",
            Tab::Image => "Image",
            Tab::File => "File",
        }
    }

    /// Whether the viewer shows the **enclosing** frame — the whole
    /// straightened source — rather than the cropped result while this tab is
    /// open.
    ///
    /// One rule, read twice: it is what puts the crop overlay over the picture
    /// and what is handed to `Session::set_cropping`. Written here rather than
    /// as two comparisons in a shell because the two must not be able to
    /// disagree — a rectangle drawn over a viewer that is showing the crop
    /// rather than the frame around it is a rectangle with nothing outside it
    /// to drag back in, and cropping left on after switching away shows the
    /// wrong picture on every other tab.
    pub fn shows_whole_frame(self) -> bool {
        self == Tab::Image
    }
}

/// One collapsing section of the Colour tab.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    Curves,
    Basic,
    ColourWarper,
    ColourWheels,
    ColourMixer,
}

impl Section {
    /// Every section, in the order the Colour tab draws them.
    ///
    /// Not the order they were listed in when each was its own tool, and the
    /// difference is deliberate: the curve carries the histogram, so there is
    /// one histogram rather than two and it is at the top, where a histogram
    /// belongs.
    pub const ALL: [Section; 5] = [
        Section::Curves,
        Section::Basic,
        Section::ColourWarper,
        Section::ColourWheels,
        Section::ColourMixer,
    ];

    /// What the heading says.
    ///
    /// `Primaries - Color Wheels` is Resolve's own label, hyphen and American
    /// spelling included. It is a proper noun here rather than a description,
    /// which is why it is not tidied into the spelling the rest of the
    /// application uses.
    pub fn title(self) -> &'static str {
        match self {
            Section::Curves => "Curves",
            Section::Basic => "Basic",
            Section::ColourWarper => "Colour Warper",
            Section::ColourWheels => "Primaries - Color Wheels",
            Section::ColourMixer => "Colour Mixer",
        }
    }

    /// Whether the section is open the first time it is seen.
    ///
    /// The warper and the mixer are shut, and both for the same reason: each is
    /// large, occasional, and drawn from a grid or a set of six wheels rather
    /// than a few rows. A tab that opens everything is the scrolling problem
    /// the tabs exist to solve.
    ///
    /// Only the *first* time — a shell remembers what the reader folded.
    pub fn starts_open(self) -> bool {
        !matches!(self, Section::ColourWarper | Section::ColourMixer)
    }

    /// The pinned effects this section draws, in the order it draws them.
    ///
    /// The six under Basic are the Lightroom-ish ones, kept in
    /// [`crate::PINNED_ROWS`] order — neutralise the light, set the exposure,
    /// then shape the tone, the detail, and finally the colour.
    pub fn effects(self) -> &'static [&'static str] {
        match self {
            Section::Curves => &["curves"],
            Section::Basic => &[
                "white_balance",
                "exposure",
                "contrast",
                "tone",
                "presence",
                "colour",
            ],
            Section::ColourWarper => &["colour_warper"],
            Section::ColourWheels => &["primaries", "log_wheels"],
            Section::ColourMixer => &["colour_mixer"],
        }
    }

    /// The section that draws an effect, if a pinned one does.
    ///
    /// `None` is the honest answer for an added effect: it is drawn by the
    /// Effects tab as one of the stack's rows, not as a fixed panel of its own.
    pub fn of(effect: &str) -> Option<Section> {
        Section::ALL
            .into_iter()
            .find(|s| s.effects().contains(&effect))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Four tabs, in the Windows shell's order. `ALL` is a hand-written list
    /// beside an enum, so the exhaustive match is what fails to compile when a
    /// variant is added and not listed.
    #[test]
    fn all_lists_every_tab() {
        for tab in Tab::ALL {
            match tab {
                Tab::Colour | Tab::Effects | Tab::Image | Tab::File => {}
            }
        }
        assert_eq!(
            Tab::ALL.map(|t| t.name()),
            ["Colour", "Effects", "Image", "File"]
        );
    }

    /// The five sections of the Colour tab, in `main.rs`'s order — which is not
    /// the order they were listed in when each was its own tool. Curves first,
    /// because it carries the histogram.
    #[test]
    fn the_colour_tab_is_five_sections_in_the_windows_order() {
        assert_eq!(
            Section::ALL.map(|s| s.title()),
            [
                "Curves",
                "Basic",
                "Colour Warper",
                "Primaries - Color Wheels",
                "Colour Mixer",
            ]
        );
    }

    /// Two are shut to begin with, and both because they are large and
    /// occasional.
    #[test]
    fn the_warper_and_the_mixer_start_shut() {
        for section in Section::ALL {
            let should = !matches!(section, Section::ColourWarper | Section::ColourMixer);
            assert_eq!(section.starts_open(), should, "{section:?}");
        }
    }

    /// Every pinned effect is drawn by exactly one section. One drawn by none
    /// is a row of the document that appears nowhere, with nothing to say so;
    /// one drawn by two is the same controls in two places, disagreeing the
    /// moment either is used.
    #[test]
    fn every_pinned_effect_belongs_to_exactly_one_section() {
        for key in crate::PINNED_ROWS {
            let owners: Vec<Section> = Section::ALL
                .into_iter()
                .filter(|s| s.effects().contains(key))
                .collect();
            assert_eq!(owners.len(), 1, "{key} is drawn by {owners:?}");
        }
    }

    /// And no section claims a key that is not a pinned row — which is what a
    /// section left holding a key looks like after the row it named stopped
    /// being pinned.
    #[test]
    fn no_section_claims_an_unpinned_effect() {
        for section in Section::ALL {
            for key in section.effects() {
                assert!(
                    crate::PINNED_ROWS.contains(key),
                    "{section:?} claims {key}, which is not pinned"
                );
            }
        }
    }

    #[test]
    fn a_pinned_effect_finds_its_section_and_an_added_one_does_not() {
        assert_eq!(Section::of("curves"), Some(Section::Curves));
        assert_eq!(Section::of("log_wheels"), Some(Section::ColourWheels));
        assert_eq!(Section::of("halation"), None);
    }

    #[test]
    fn only_the_image_tab_shows_the_whole_frame() {
        for tab in Tab::ALL {
            assert_eq!(tab.shows_whole_frame(), tab == Tab::Image, "{tab:?}");
        }
    }

    #[test]
    fn the_inspector_opens_on_colour() {
        assert_eq!(Tab::default(), Tab::Colour);
    }
}
