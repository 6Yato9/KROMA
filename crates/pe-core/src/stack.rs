//! The effect stack — an ordered list of rows, each of which is a node in
//! disguise.
//!
//! In Resolve, a node has an RGB input, a key input, an opacity and a composite
//! mode. Every row here has the same anatomy, and that is the single most
//! important decision in this file. Because the key and the blend live on the
//! *row* rather than inside particular effects, "grain at 40% in Screen mode,
//! sky only" needs no special code anywhere — it is three fields that already
//! exist. Put them inside individual effects instead and each becomes a
//! separate feature request against every effect in the registry, forever.

use serde::{Deserialize, Serialize};

use crate::params::ParamMap;

/// Stable identifier for a row. Survives reordering, which is why the UI keys
/// off this and never off the index.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RowId(pub u64);

/// How a row's result is combined with what came before it.
///
/// The set Resolve exposes on a node, minus the ones that only make sense with
/// an alpha channel we do not have.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BlendMode {
    #[default]
    Normal,
    Add,
    Multiply,
    Screen,
    Overlay,
    SoftLight,
    HardLight,
    Darken,
    Lighten,
    Difference,
    Exclusion,
    /// Takes hue and saturation from the result, luminance from the input.
    Color,
    /// Takes luminance from the result, hue and saturation from the input.
    Luminosity,
}

impl BlendMode {
    pub const ALL: &'static [BlendMode] = &[
        BlendMode::Normal,
        BlendMode::Add,
        BlendMode::Multiply,
        BlendMode::Screen,
        BlendMode::Overlay,
        BlendMode::SoftLight,
        BlendMode::HardLight,
        BlendMode::Darken,
        BlendMode::Lighten,
        BlendMode::Difference,
        BlendMode::Exclusion,
        BlendMode::Color,
        BlendMode::Luminosity,
    ];

    /// Blend modes that model light arriving at a sensor and should therefore
    /// be evaluated in linear space regardless of what the effect above them
    /// asked for. The rest are perceptual and belong in log.
    ///
    /// This distinction is the two-space rule applied to compositing, and it is
    /// the reason `Screen`-mode halation looks like glow rather than haze.
    pub fn is_light_like(self) -> bool {
        matches!(self, BlendMode::Add | BlendMode::Screen)
    }

    /// Index the shader switches on.
    ///
    /// Derived from declaration order rather than written out, so the two
    /// cannot drift apart silently — and pinned by a test against the constants
    /// in `shaders/common.wgsl`.
    pub fn as_index(self) -> u32 {
        BlendMode::ALL
            .iter()
            .position(|m| *m == self)
            .expect("every variant is in ALL") as u32
    }

    pub fn as_str(self) -> &'static str {
        match self {
            BlendMode::Normal => "Normal",
            BlendMode::Add => "Add",
            BlendMode::Multiply => "Multiply",
            BlendMode::Screen => "Screen",
            BlendMode::Overlay => "Overlay",
            BlendMode::SoftLight => "Soft Light",
            BlendMode::HardLight => "Hard Light",
            BlendMode::Darken => "Darken",
            BlendMode::Lighten => "Lighten",
            BlendMode::Difference => "Difference",
            BlendMode::Exclusion => "Exclusion",
            BlendMode::Color => "Color",
            BlendMode::Luminosity => "Luminosity",
        }
    }
}

/// The key (mask) that limits a row's effect to part of the image.
///
/// M0 defines the shape only; the variants are implemented at M3. Declaring
/// them now means the document format does not need a migration when they
/// land, and it forces the renderer's row loop to handle an optional key from
/// the very first version.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Key {
    /// Resolve's power windows.
    Window(WindowShape),
    /// HSL qualifier.
    Qualifier(ParamMap),
    /// Painted mask, stored as strokes so it stays resolution-independent.
    Brush(ParamMap),
    /// Machine-generated: subject, sky, skin, depth.
    Generated { source: String, params: ParamMap },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WindowShape {
    Linear,
    Circle,
    Polygon,
    Curve,
    Gradient,
}

/// Modifiers applied to a key after it is generated. Resolve keeps these in the
/// Key palette; they apply uniformly to every kind of key, so they live here
/// rather than inside each variant.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct KeyAdjust {
    pub invert: bool,
    /// Multiplies the matte. Resolve calls this Key Output Gain.
    pub gain: f32,
    /// Added to the matte before gain.
    pub offset: f32,
    /// Blur radius in image-relative units, not pixels.
    pub softness: f32,
}

impl Default for KeyAdjust {
    fn default() -> Self {
        Self {
            invert: false,
            gain: 1.0,
            offset: 0.0,
            softness: 0.0,
        }
    }
}

/// One row of the stack.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StackRow {
    pub id: RowId,
    /// Registry key of the effect, e.g. `"exposure"`. A string rather than an
    /// enum so that a document containing an effect this build does not know
    /// about can still round-trip instead of failing to open.
    pub effect: String,
    #[serde(default)]
    pub params: ParamMap,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_one")]
    pub opacity: f32,
    #[serde(default)]
    pub blend: BlendMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<Key>,
    #[serde(default)]
    pub key_adjust: KeyAdjust,
    /// User-visible label. `None` means "use the effect's display name".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// A fixed panel rather than a row the user added.
    ///
    /// Lightroom's Basic panel is always there — you do not add Exposure, it
    /// exists, sitting at zero. That looks incompatible with a stack and is
    /// not: a fixed panel is simply a row created with the document that
    /// cannot be deleted or reordered. The renderer, the cache and export see
    /// one ordered list and know nothing about panels.
    ///
    /// Pinned rows always occupy the head of the stack, so user rows are
    /// always applied after them.
    #[serde(default)]
    pub pinned: bool,
}

fn default_true() -> bool {
    true
}
fn default_one() -> f32 {
    1.0
}

impl StackRow {
    pub fn new(id: RowId, effect: impl Into<String>) -> Self {
        Self {
            id,
            effect: effect.into(),
            params: ParamMap::default(),
            enabled: true,
            opacity: 1.0,
            blend: BlendMode::Normal,
            key: None,
            key_adjust: KeyAdjust::default(),
            label: None,
            pinned: false,
        }
    }

    /// A fixed panel row, created with the document.
    pub fn pinned(id: RowId, effect: impl Into<String>) -> Self {
        Self {
            pinned: true,
            ..Self::new(id, effect)
        }
    }

    /// Whether this row can be skipped entirely by the renderer.
    ///
    /// A disabled row and a fully transparent row are the same thing as far as
    /// output goes, and skipping them keeps a long stack cheap when the user is
    /// A/B-ing with the enable toggles.
    pub fn is_noop(&self) -> bool {
        !self.enabled || self.opacity <= 0.0
    }
}

/// The ordered list of rows.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Stack {
    pub rows: Vec<StackRow>,
}

impl Stack {
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    pub fn iter(&self) -> std::slice::Iter<'_, StackRow> {
        self.rows.iter()
    }

    /// Add a row to the end.
    ///
    /// Ids must be unique. Every lookup here is a linear scan that stops at
    /// the first match, so a duplicate does not error — it silently resolves
    /// to whichever row was pushed first, and the other one becomes a row you
    /// can see and cannot touch. The assertion fires where the mistake is
    /// made rather than where it is felt.
    pub fn push(&mut self, row: StackRow) {
        debug_assert!(
            !self.rows.iter().any(|r| r.id == row.id),
            "row id {} is already in the stack",
            row.id.0
        );
        self.rows.push(row);
    }

    /// How many pinned rows sit at the head of the stack.
    ///
    /// User rows begin here, and reordering is confined to that range.
    pub fn pinned_count(&self) -> usize {
        self.rows.iter().take_while(|r| r.pinned).count()
    }

    pub fn insert(&mut self, index: usize, row: StackRow) {
        self.rows.insert(index.min(self.rows.len()), row);
    }

    /// Remove a row. Pinned rows refuse — they are panels, not entries.
    pub fn remove(&mut self, id: RowId) -> Option<StackRow> {
        let i = self.index_of(id)?;
        if self.rows[i].pinned {
            return None;
        }
        Some(self.rows.remove(i))
    }

    pub fn index_of(&self, id: RowId) -> Option<usize> {
        self.rows.iter().position(|r| r.id == id)
    }

    /// The first row running a given effect.
    ///
    /// How a fixed panel finds the row it drives: the Basic panel knows it
    /// owns `exposure`, not which index that landed at.
    pub fn find_by_effect(&self, effect: &str) -> Option<RowId> {
        self.rows.iter().find(|r| r.effect == effect).map(|r| r.id)
    }

    pub fn get(&self, id: RowId) -> Option<&StackRow> {
        self.rows.iter().find(|r| r.id == id)
    }

    pub fn get_mut(&mut self, id: RowId) -> Option<&mut StackRow> {
        self.rows.iter_mut().find(|r| r.id == id)
    }

    /// Move a row to a new index. Returns the range of indices whose rendered
    /// output is now invalid, which the stage cache uses to decide how much to
    /// throw away.
    /// Move a row. Pinned rows do not move, and user rows cannot be dragged
    /// above them — the fixed panels are always applied first.
    pub fn reorder(&mut self, id: RowId, to: usize) -> Option<usize> {
        let from = self.index_of(id)?;
        if self.rows[from].pinned {
            return Some(from);
        }
        let floor = self.pinned_count();
        let to = to.clamp(floor, self.rows.len().saturating_sub(1));
        if from == to {
            return Some(from);
        }
        let row = self.rows.remove(from);
        self.rows.insert(to, row);
        Some(from.min(to))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stack_of(n: u64) -> Stack {
        let mut s = Stack::default();
        for i in 0..n {
            s.push(StackRow::new(RowId(i), format!("effect{i}")));
        }
        s
    }

    #[test]
    fn a_new_row_is_enabled_opaque_and_unmasked() {
        // These defaults are load-bearing: adding an effect must visibly do
        // something immediately, or the UI feels broken.
        let r = StackRow::new(RowId(1), "exposure");
        assert!(r.enabled);
        assert_eq!(r.opacity, 1.0);
        assert_eq!(r.blend, BlendMode::Normal);
        assert!(r.key.is_none());
        assert!(!r.is_noop());
    }

    #[test]
    fn disabled_or_transparent_rows_are_noops() {
        let mut r = StackRow::new(RowId(1), "grain");
        r.enabled = false;
        assert!(r.is_noop());

        let mut r = StackRow::new(RowId(2), "grain");
        r.opacity = 0.0;
        assert!(r.is_noop());
    }

    #[test]
    fn reorder_reports_the_earliest_dirty_index() {
        let mut s = stack_of(5);
        // Moving row 3 up to position 1 invalidates everything from 1 onward.
        assert_eq!(s.reorder(RowId(3), 1), Some(1));
        let order: Vec<_> = s.iter().map(|r| r.id.0).collect();
        assert_eq!(order, vec![0, 3, 1, 2, 4]);

        // Moving row 1 down to 3 invalidates from 1 onward too.
        let mut s = stack_of(5);
        assert_eq!(s.reorder(RowId(1), 3), Some(1));
        let order: Vec<_> = s.iter().map(|r| r.id.0).collect();
        assert_eq!(order, vec![0, 2, 3, 1, 4]);
    }

    #[test]
    fn reordering_to_the_same_place_is_a_noop() {
        let mut s = stack_of(3);
        let before = s.clone();
        assert_eq!(s.reorder(RowId(1), 1), Some(1));
        assert_eq!(s, before);
    }

    #[test]
    fn reorder_past_the_end_clamps() {
        let mut s = stack_of(3);
        assert!(s.reorder(RowId(0), 99).is_some());
        let order: Vec<_> = s.iter().map(|r| r.id.0).collect();
        assert_eq!(order, vec![1, 2, 0]);
    }

    #[test]
    fn ids_survive_reordering() {
        let mut s = stack_of(4);
        s.reorder(RowId(0), 3);
        assert!(s.get(RowId(0)).is_some());
        assert_eq!(s.index_of(RowId(0)), Some(3));
    }

    #[test]
    fn a_pinned_row_cannot_be_deleted() {
        let mut s = Stack::default();
        s.push(StackRow::pinned(RowId(0), "exposure"));
        assert!(s.remove(RowId(0)).is_none());
        assert_eq!(s.len(), 1);
    }

    #[test]
    fn a_pinned_row_cannot_be_moved() {
        let mut s = Stack::default();
        s.push(StackRow::pinned(RowId(0), "exposure"));
        s.push(StackRow::pinned(RowId(1), "contrast"));
        s.push(StackRow::new(RowId(2), "grain"));
        s.reorder(RowId(0), 2);
        let order: Vec<_> = s.iter().map(|r| r.id.0).collect();
        assert_eq!(order, vec![0, 1, 2]);
    }

    #[test]
    fn a_user_row_cannot_be_dragged_above_the_pinned_panels() {
        // The fixed panels are always applied first. Letting grain sit above
        // Exposure would make the inspector lie about the order of operations.
        let mut s = Stack::default();
        s.push(StackRow::pinned(RowId(0), "exposure"));
        s.push(StackRow::pinned(RowId(1), "contrast"));
        s.push(StackRow::new(RowId(2), "grain"));
        s.push(StackRow::new(RowId(3), "halation"));

        s.reorder(RowId(3), 0);
        let order: Vec<_> = s.iter().map(|r| r.id.0).collect();
        assert_eq!(order, vec![0, 1, 3, 2], "halation should stop at the floor");
        assert_eq!(s.pinned_count(), 2);
    }

    #[test]
    fn pinned_state_survives_a_save() {
        let row = StackRow::pinned(RowId(1), "exposure");
        let json = serde_json::to_string(&row).unwrap();
        let back: StackRow = serde_json::from_str(&json).unwrap();
        assert!(back.pinned);
    }

    #[test]
    fn rows_from_before_pinning_existed_load_as_user_rows() {
        let json = r#"{"id":7,"effect":"grain"}"#;
        let row: StackRow = serde_json::from_str(json).unwrap();
        assert!(!row.pinned);
    }

    #[test]
    fn add_and_screen_are_the_light_like_modes() {
        for m in BlendMode::ALL {
            assert_eq!(
                m.is_light_like(),
                matches!(m, BlendMode::Add | BlendMode::Screen),
                "{m:?}"
            );
        }
    }

    /// The Rust enum and the WGSL constants are one ABI across two languages,
    /// and nothing but this test connects them. Reordering `BlendMode` without
    /// editing the shader would silently turn every Screen-mode row into
    /// Overlay in every saved document.
    #[test]
    fn blend_mode_indices_match_the_shader() {
        let shader = include_str!("../../../shaders/common.wgsl");

        for mode in BlendMode::ALL {
            // Normal -> BLEND_NORMAL, SoftLight -> BLEND_SOFT_LIGHT
            let mut name = String::from("BLEND");
            for ch in format!("{mode:?}").chars() {
                if ch.is_uppercase() {
                    name.push('_');
                }
                name.push(ch.to_ascii_uppercase());
            }

            let decl = format!("const {name}: u32 = ");
            let line = shader
                .lines()
                .find(|l| l.trim_start().starts_with(&decl))
                .unwrap_or_else(|| panic!("{name} is missing from common.wgsl"));

            let value: u32 = line
                .rsplit_once("= ")
                .and_then(|(_, v)| v.trim().trim_end_matches(&['u', ';'][..]).parse().ok())
                .unwrap_or_else(|| panic!("could not parse {line:?}"));

            assert_eq!(
                mode.as_index(),
                value,
                "{mode:?} is {} in Rust but {name} is {value} in the shader",
                mode.as_index()
            );
        }
    }

    #[test]
    fn indices_are_unique_and_dense() {
        let mut seen: Vec<u32> = BlendMode::ALL.iter().map(|m| m.as_index()).collect();
        seen.sort();
        assert_eq!(seen, (0..BlendMode::ALL.len() as u32).collect::<Vec<_>>());
    }
}
