//! Which tool an effect belongs to.
//!
//! Resolve's colour page shows one tool at a time, chosen from a strip of
//! icons, and eleven pinned panels in one scrolling column is the thing that
//! arrangement exists to avoid: reaching the warper means scrolling past a
//! hundred and thirty controls, and every title on the way wears the accent at
//! once.
//!
//! Shared rather than written per shell because it is one answer per effect.
//! The Windows shell currently spells its own version as five collapsing
//! headers in a match arm, which is the shape that drifts the first time a
//! twelfth pinned effect appears.

/// One page of the strip, in the order it shows them.
///
/// Mostly colour tools, and then the two that are not: Crop edits the
/// document's geometry, and File is about the file. The strip is this shell's
/// answer to the Windows shell's four tabs, so it carries what they carry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Basic,
    ColourWheels,
    Curves,
    ColourWarper,
    ColourMixer,
    /// Whatever the user added, and the browser that adds more.
    Effects,
    /// The crop, the straightening angle, the quarter-turns and the flips.
    ///
    /// The one tool that edits the document's *geometry* rather than a row in
    /// its stack, which is why it owns no pinned effects: there is no
    /// `Effect` behind it and no parameters to look up. It follows Effects for
    /// the same reason the Windows shell puts its Image page after its Effects
    /// tab.
    Crop,
    /// What the photograph is, and what it will be written as.
    ///
    /// The one tool about the *file* rather than the picture. It owns no
    /// pinned effects for the same reason Crop does not — there is no `Effect`
    /// behind it — and it sits last, in the order the Windows shell lists its
    /// tabs: Colour, Effects, Image, File.
    File,
}

impl Tool {
    /// Every tool, in the order the strip shows them.
    ///
    /// Here rather than in a shell for the same reason [`crate::Group::ALL`]
    /// is: a variant added and not listed is a tool with no button, and the
    /// only symptom is a panel nobody can reach.
    pub const ALL: [Tool; 8] = [
        Tool::Basic,
        Tool::ColourWheels,
        Tool::Curves,
        Tool::ColourWarper,
        Tool::ColourMixer,
        Tool::Effects,
        Tool::Crop,
        Tool::File,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Tool::Basic => "Basic",
            Tool::ColourWheels => "Colour Wheels",
            Tool::Curves => "Curves",
            Tool::ColourWarper => "Colour Warper",
            Tool::ColourMixer => "Colour Mixer",
            Tool::Effects => "Effects",
            Tool::Crop => "Crop",
            Tool::File => "File",
        }
    }

    /// The pinned effects this tool draws, in the order it draws them.
    ///
    /// The six under Basic are the Lightroom-ish ones the Windows shell
    /// already gathers under a single header, kept in
    /// [`crate::PINNED_ROWS`] order — neutralise the light, set the exposure,
    /// then shape the tone, the detail, and finally the colour.
    pub fn effects(self) -> &'static [&'static str] {
        match self {
            Tool::Basic => &[
                "white_balance",
                "exposure",
                "contrast",
                "tone",
                "presence",
                "colour",
            ],
            Tool::ColourWheels => &["primaries", "log_wheels"],
            Tool::Curves => &["curves"],
            Tool::ColourWarper => &["colour_warper"],
            Tool::ColourMixer => &["colour_mixer"],
            // None of these three is pinned, and deliberately. Effects shows
            // the rows the user put there; Crop edits the document's geometry,
            // which is not a row at all; File is about the file rather than
            // the picture.
            Tool::Effects => &[],
            Tool::Crop => &[],
            Tool::File => &[],
        }
    }

    /// The tool that draws an effect, if a pinned one does.
    ///
    /// `None` is the honest answer for an added effect: it is drawn by
    /// [`Tool::Effects`] as one of the stack's rows, not as a fixed panel of
    /// its own.
    pub fn of(effect: &str) -> Option<Tool> {
        Tool::ALL
            .into_iter()
            .find(|t| t.effects().contains(&effect))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every pinned effect has exactly one home. A pinned effect belonging to
    /// no tool is one the user cannot reach at all once the panel shows one
    /// tool at a time — it would simply not be drawn, anywhere, with nothing
    /// to say so.
    /// How many rows a fresh document carries.
    ///
    /// Asserted not because the number itself matters but because it is quoted
    /// in prose that reasons from it — the pass counter's justification in
    /// `pe-effects`, the neutral-skip in `pe-render`, the export's VRAM
    /// arithmetic, and two comments in the Swift shell about how many accented
    /// titles is too many. It read "nine" for a long time after `colour_warper`
    /// and `log_wheels` were pinned. If this fails, go and read those.
    #[test]
    fn a_fresh_document_carries_eleven_rows() {
        assert_eq!(
            crate::PINNED_ROWS.len(),
            11,
            "the pinned count changed; four comments and two Swift files quote it"
        );
    }

    #[test]
    fn every_pinned_effect_belongs_to_exactly_one_tool() {
        for key in crate::PINNED_ROWS {
            let homes: Vec<Tool> = Tool::ALL
                .iter()
                .copied()
                .filter(|t| t.effects().contains(key))
                .collect();
            assert_eq!(homes.len(), 1, "{key} has {} homes", homes.len());
        }
    }

    /// And no tool claims an effect that is not registered, which is what a
    /// renamed key looks like.
    #[test]
    fn no_tool_claims_an_effect_that_does_not_exist() {
        for tool in Tool::ALL {
            for key in tool.effects() {
                assert!(
                    crate::by_key(key).is_some(),
                    "{tool:?} claims {key}, which is not a registered effect"
                );
            }
        }
    }

    /// The other half of the same question: a tool must not claim an effect
    /// that exists but is not pinned. A fixed panel for a row the document
    /// does not start with is a panel with nothing behind it.
    #[test]
    fn no_tool_claims_an_effect_that_is_not_pinned() {
        for tool in Tool::ALL {
            for key in tool.effects() {
                assert!(
                    crate::PINNED_ROWS.contains(key),
                    "{tool:?} claims {key}, which is not a pinned row"
                );
            }
        }
    }

    /// Exactly two tools have no pinned effects of their own, and both are
    /// deliberate: [`Tool::Effects`] shows whatever the user put there, and
    /// [`Tool::Crop`] edits the document's geometry rather than a row in the
    /// stack.
    ///
    /// Named rather than counted, and asserted in both directions, because the
    /// failure this guards is a *third* one — a tool added to the strip and its
    /// effects list forgotten. That draws an empty panel with nothing to say
    /// so, and the effects it should have drawn appear nowhere: the same
    /// silence `every_pinned_effect_belongs_to_exactly_one_tool` catches from
    /// the other end.
    #[test]
    fn only_effects_crop_and_file_own_nothing_pinned() {
        let empty: Vec<Tool> = Tool::ALL
            .into_iter()
            .filter(|t| t.effects().is_empty())
            .collect();
        assert_eq!(
            empty,
            vec![Tool::Effects, Tool::Crop, Tool::File],
            "a tool with no pinned effects that is not one of these three is a \
             panel that draws nothing"
        );
    }

    #[test]
    fn the_strip_opens_on_basic() {
        assert_eq!(Tool::ALL.first().copied(), Some(Tool::Basic));
    }

    /// An added effect has no fixed panel, and saying so is what lets a caller
    /// draw it as a stack row rather than hunt for a tool that does not exist.
    #[test]
    fn an_effect_nobody_pinned_has_no_tool() {
        assert_eq!(Tool::of("colour_warper"), Some(Tool::ColourWarper));
        assert_eq!(Tool::of("dehaze"), None);
        assert_eq!(Tool::of("not_an_effect"), None);
    }

    /// And no tool is listed twice, which would draw it twice in the strip.
    #[test]
    fn the_strip_has_no_repeats() {
        let mut seen = Tool::ALL.to_vec();
        seen.sort_by_key(|t| t.name());
        seen.dedup();
        assert_eq!(seen.len(), Tool::ALL.len());
    }
}
