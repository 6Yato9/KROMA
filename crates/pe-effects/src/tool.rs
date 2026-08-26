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

/// One page of the colour tools, in the strip's order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Basic,
    ColourWheels,
    Curves,
    ColourWarper,
    ColourMixer,
    /// Whatever the user added, and the browser that adds more.
    Effects,
}

impl Tool {
    /// Every tool, in the order the strip shows them.
    ///
    /// Here rather than in a shell for the same reason [`crate::Group::ALL`]
    /// is: a variant added and not listed is a tool with no button, and the
    /// only symptom is a panel nobody can reach.
    pub const ALL: [Tool; 6] = [
        Tool::Basic,
        Tool::ColourWheels,
        Tool::Curves,
        Tool::ColourWarper,
        Tool::ColourMixer,
        Tool::Effects,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Tool::Basic => "Basic",
            Tool::ColourWheels => "Colour Wheels",
            Tool::Curves => "Curves",
            Tool::ColourWarper => "Colour Warper",
            Tool::ColourMixer => "Colour Mixer",
            Tool::Effects => "Effects",
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
            // Not pinned, and deliberately: this one shows the rows the user
            // put there.
            Tool::Effects => &[],
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

    /// The added stack is a tool with no pinned effects of its own — it shows
    /// whatever the user put there.
    #[test]
    fn the_effects_tool_owns_nothing_pinned() {
        assert!(Tool::Effects.effects().is_empty());
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
