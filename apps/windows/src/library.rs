//! The set of photographs open at once, and the edit belonging to each.
//!
//! Only one photograph is decoded at a time. A 24-megapixel frame is 96 MB of
//! RGBA, so a folder of two hundred would be twenty gigabytes — the whole
//! reason a filmstrip exists is to make a set navigable *without* holding it.
//! What the library keeps per photo is a path, a few kilobytes of edit, and a
//! 128-pixel thumbnail.
//!
//! Thumbnails are decoded on a worker thread and arrive over a channel. The
//! alternative is decoding them all when a folder is opened, which for two
//! hundred JPEGs is half a minute of frozen window before anything can be
//! done — and the first thing anyone does is click the photo they were looking
//! for, which needs none of the others.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};

use pe_core::{Document, History, RowIdGenerator};
use pe_session::{Support, autosave};

/// Long edge of a filmstrip thumbnail, in pixels.
///
/// Big enough to recognise a frame at a glance and to survive a high-DPI
/// display; small enough that a thousand of them is 200 MB rather than 20 GB.
pub const THUMB_EDGE: u32 = 128;

/// The extensions the open dialogs offer and a folder scan accepts.
pub const EXTENSIONS: [&str; 3] = ["jpg", "jpeg", "png"];

/// One photograph in the set.
pub struct Entry {
    pub path: PathBuf,
    /// The edit, parked while a different photograph is being worked on.
    ///
    /// The whole history rather than just the document, so that switching away
    /// and back does not quietly throw away an undo stack. `None` means this
    /// photo has never been opened and gets a fresh document when it is.
    parked: Option<(History, RowIdGenerator)>,
    pub thumb: Option<egui::TextureHandle>,
    /// The decode has been asked for, so it is not asked for again.
    requested: bool,
    pub failed: bool,
}

impl Entry {
    pub fn name(&self) -> String {
        self.path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| self.path.to_string_lossy().to_string())
    }

    /// Whether this photo has been edited, for the mark on the filmstrip.
    ///
    /// A photo that has never been opened has no parked edit and is therefore
    /// untouched; one that has been opened counts as edited only if something
    /// in it can actually be undone.
    pub fn edited(&self) -> bool {
        self.parked.as_ref().is_some_and(|(h, _)| h.can_undo())
    }

    pub fn document(&self) -> Option<&pe_core::Document> {
        self.parked.as_ref().map(|(h, _)| h.document())
    }
}

/// What the worker sends back. Raw pixels, not a texture: uploading needs the
/// egui context, which belongs to the main thread.
struct Decoded {
    index: usize,
    image: Option<pe_io::DecodedImage>,
}

pub struct Library {
    entries: Vec<Entry>,
    current: usize,
    jobs: Sender<(usize, PathBuf)>,
    done: Receiver<Decoded>,
    /// Where the autosave store for these photographs' edits lives. Given by
    /// the shell at construction rather than guessed, same as everywhere else
    /// `Support` appears.
    support: Support,
}

impl Library {
    pub fn new(paths: Vec<PathBuf>, support: Support) -> Self {
        let (jobs, rx) = channel::<(usize, PathBuf)>();
        let (tx, done) = channel::<Decoded>();

        // One worker, not a pool. Decoding is disc-bound before it is
        // CPU-bound, and a single thread keeps the thumbnails arriving in the
        // order they are shown rather than scattered through the strip.
        std::thread::Builder::new()
            .name("thumbnails".into())
            .spawn(move || {
                for (index, path) in rx {
                    let image = pe_io::load(&path)
                        .ok()
                        .map(|img| pe_io::thumbnail(&img, THUMB_EDGE));
                    // A closed channel means the window has gone; there is
                    // nothing left to deliver to.
                    if tx.send(Decoded { index, image }).is_err() {
                        break;
                    }
                }
            })
            .expect("spawn thumbnail worker");

        let entries = paths
            .into_iter()
            .map(|path| Entry {
                path,
                parked: None,
                thumb: None,
                requested: false,
                failed: false,
            })
            .collect();

        Self {
            entries,
            current: 0,
            jobs,
            done,
            support,
        }
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn current(&self) -> usize {
        self.current
    }

    /// Point the set at one of its entries without loading anything.
    ///
    /// Used once, at startup: the caller has already opened the photograph
    /// itself, and this is only telling the strip which one is showing.
    pub fn focus(&mut self, index: usize) {
        if index < self.entries.len() {
            self.current = index;
        }
    }

    /// Every path in the set, in order.
    pub fn paths(&self) -> Vec<&Path> {
        self.entries.iter().map(|e| e.path.as_path()).collect()
    }

    /// Where a photograph sits in the set, if it is still in it.
    ///
    /// A linear walk. The alternative is an index kept in step with every
    /// insertion and removal, which is the bug this exists to avoid rather
    /// than a faster version of it — and a set is tens of photographs, not
    /// millions.
    pub fn index_of(&self, path: &Path) -> Option<usize> {
        self.entries.iter().position(|e| e.path == path)
    }

    pub fn path(&self, index: usize) -> Option<&Path> {
        self.entries.get(index).map(|e| e.path.as_path())
    }

    /// Add photographs, skipping any already in the set.
    ///
    /// Returns the index of the first one that is new, which is what the
    /// caller selects — opening files and landing on none of them would be a
    /// strange thing for a program to do.
    pub fn add(&mut self, paths: impl IntoIterator<Item = PathBuf>) -> Option<usize> {
        let mut first = None;
        for path in paths {
            if self.entries.iter().any(|e| e.path == path) {
                continue;
            }
            first.get_or_insert(self.entries.len());
            self.entries.push(Entry {
                path,
                parked: None,
                thumb: None,
                requested: false,
                failed: false,
            });
        }
        first
    }

    /// Every supported image in a directory, in name order.
    ///
    /// Sorted because a directory listing is in whatever order the filesystem
    /// felt like, and a filmstrip that shuffles between runs is unusable.
    pub fn scan(dir: &Path) -> Vec<PathBuf> {
        let Ok(read) = std::fs::read_dir(dir) else {
            return Vec::new();
        };
        let mut found: Vec<PathBuf> = read
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                p.extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|e| EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
            })
            .collect();
        found.sort();
        found
    }

    /// Ask for the thumbnails that have not been asked for yet.
    ///
    /// Called with the range the strip is actually showing, so that opening a
    /// folder of a thousand does not queue a thousand decodes before the first
    /// one the user can see.
    pub fn request(&mut self, range: std::ops::Range<usize>) {
        for i in range.start..range.end.min(self.entries.len()) {
            let entry = &mut self.entries[i];
            if entry.requested || entry.thumb.is_some() {
                continue;
            }
            entry.requested = true;
            let _ = self.jobs.send((i, entry.path.clone()));
        }
    }

    /// Take delivery of whatever the worker has finished. Returns true if
    /// anything arrived, so the caller knows to repaint.
    pub fn collect(&mut self, ctx: &egui::Context) -> bool {
        let mut any = false;
        while let Ok(decoded) = self.done.try_recv() {
            any = true;
            let Some(entry) = self.entries.get_mut(decoded.index) else {
                continue;
            };
            match decoded.image {
                Some(img) => {
                    let colour = egui::ColorImage {
                        size: [img.width as usize, img.height as usize],
                        pixels: img
                            .pixels
                            .as_chunks::<4>()
                            .0
                            .iter()
                            .map(|p| egui::Color32::from_rgb(p[0], p[1], p[2]))
                            .collect(),
                        source_size: egui::vec2(img.width as f32, img.height as f32),
                    };
                    entry.thumb = Some(ctx.load_texture(
                        entry.path.to_string_lossy(),
                        colour,
                        egui::TextureOptions::LINEAR,
                    ));
                }
                None => entry.failed = true,
            }
        }
        any
    }

    /// Park the current photo's edit and take the one belonging to `index`.
    ///
    /// The caller is responsible for the pixels; this moves only the edit,
    /// which is the part that must not be lost.
    ///
    /// `declared` is the colour space the incoming photograph's file claims,
    /// which only matters if this is the first time it has been opened — see
    /// [`fresh_document`].
    pub fn switch(
        &mut self,
        index: usize,
        outgoing: History,
        outgoing_ids: RowIdGenerator,
        declared: Option<&'static str>,
    ) -> (History, RowIdGenerator) {
        if let Some(entry) = self.entries.get_mut(self.current) {
            entry.parked = Some((outgoing, outgoing_ids));
        }
        self.current = index.min(self.entries.len().saturating_sub(1));
        self.take_current(declared)
    }

    /// Take the edit belonging to the photograph currently pointed at,
    /// parking nothing.
    ///
    /// Separate from [`Library::switch`] for the one case where there is
    /// nothing to park: the edit in hand belonged to a photograph that has
    /// just been taken out of the set.
    pub fn take_current(&mut self, declared: Option<&'static str>) -> (History, RowIdGenerator) {
        let support = self.support.clone();
        let Some(entry) = self.entries.get_mut(self.current) else {
            return (
                History::new(Document::from_path(String::new())),
                RowIdGenerator::default(),
            );
        };
        match entry.parked.take() {
            Some(pair) => pair,
            None => {
                // Never opened. An edit saved beside it is worth honouring —
                // that is the whole point of writing one — and a fresh
                // document otherwise.
                let doc = load_edit(&support, &entry.path)
                    .unwrap_or_else(|| fresh_document(&entry.path, declared));
                let ids = RowIdGenerator::resuming(&doc);
                (History::new(doc), ids)
            }
        }
    }

    pub fn remove(&mut self, index: usize) {
        if index >= self.entries.len() {
            return;
        }
        self.entries.remove(index);
        if self.current >= self.entries.len() {
            self.current = self.entries.len().saturating_sub(1);
        } else if index < self.current {
            self.current -= 1;
        }
    }

    /// Apply an edit to every photo in the set except the one in hand.
    ///
    /// The stack only. A crop is about the frame it was drawn on, and pasting
    /// a landscape crop onto a portrait shot is almost never what anyone
    /// meant; the grade travels, the framing does not.
    pub fn paste_stack_to_all(&mut self, stack: &pe_core::Stack) -> usize {
        let current = self.current;
        let support = self.support.clone();
        let mut n = 0;
        for (i, entry) in self.entries.iter_mut().enumerate() {
            if i == current {
                continue;
            }
            let (history, ids) = entry.parked.take().unwrap_or_else(|| {
                // A photograph nobody has opened this session may still have an
                // edit saved beside it, and this used to throw it away: the
                // grade landed on a blank document, taking the crop with it.
                // Which is precisely what the paragraph above promises not to
                // do — and the promise held or not depending on whether you
                // happened to have visited the photograph.
                let doc = load_edit(&support, &entry.path)
                    .unwrap_or_else(|| fresh_document(&entry.path, None));
                let ids = RowIdGenerator::resuming(&doc);
                (History::new(doc), ids)
            });
            let mut history = history;
            let stack = stack.clone();
            history.edit("Paste Grade", None, move |doc| doc.stack = stack);
            // The pasted rows carry ids from another document, so the
            // generator has to be told where the numbering now stands or the
            // next effect added here would collide with one of them.
            let _ = ids;
            let ids = RowIdGenerator::resuming(history.document());
            entry.parked = Some((history, ids));
            n += 1;
        }
        n
    }
}

/// Read the edit saved beside a photograph, if there is one.
/// The edit to open this photograph with, if there is one.
///
/// The autosave first, then the sidecar. Not a judgement about which is more
/// important — it is that the autosave is by construction the *later* of the
/// two: saving a sidecar does not stop the work being autosaved, so anything
/// in the sidecar is also in the autosave unless you have edited since, in
/// which case the autosave is what you were actually looking at.
///
/// "Load edit" stays the explicit way to pull a sidecar back over the top.
/// A document for a photograph that has never been opened, believing what the
/// file said about itself.
///
/// The declared space is applied here and only here, at the moment a document
/// is invented. A document that already exists has an answer — either this one,
/// given to it the first time, or one a person chose afterwards — and a file's
/// claim is not grounds to overrule a person.
pub fn fresh_document(path: &Path, declared: Option<&'static str>) -> pe_core::Document {
    let mut doc = pe_effects::new_document(path.to_string_lossy().to_string());
    if let Some(space) = declared {
        doc.color.input = space.to_string();
    }
    doc
}

pub fn load_edit(support: &Support, path: &Path) -> Option<pe_core::Document> {
    if let Some(doc) = autosave::load(support, path) {
        return Some(doc);
    }
    load_sidecar(path)
}

/// Strictly the `.peproj` beside the photograph, ignoring any autosave.
pub fn load_sidecar(path: &Path) -> Option<pe_core::Document> {
    let json = std::fs::read_to_string(path.with_extension("peproj")).ok()?;
    pe_core::Document::from_json(&json).ok()
}

#[cfg(test)]
mod tests {

    /// A grade belongs to the photograph it was made on.
    ///
    /// Switching parks the outgoing edit and takes the incoming one, and the
    /// incoming one for a photograph never opened is a fresh document. Worth
    /// asserting rather than reading, because the failure is quiet and looks
    /// exactly like a colour-management bug: the new picture simply comes up
    /// looking wrong.
    /// A batch holds paths, so the set may change under it. This is the
    /// lookup that makes that safe, and the case it has to survive.
    #[test]
    fn a_photograph_keeps_its_identity_when_the_set_shifts() {
        let mut lib = Library::new(
            vec![
                PathBuf::from("a.jpg"),
                PathBuf::from("b.jpg"),
                PathBuf::from("c.jpg"),
            ],
            Support::default(),
        );
        let c = PathBuf::from("c.jpg");
        assert_eq!(lib.index_of(&c), Some(2));

        // Take one out from in front of it: every position after slides.
        lib.remove(0);
        assert_eq!(
            lib.index_of(&c),
            Some(1),
            "the lookup did not follow the photograph"
        );
        assert_eq!(lib.path(2), None, "the old position is now off the end");

        // And once it is gone it is gone, rather than resolving to a
        // neighbour — which is what an index would have done.
        lib.remove(1);
        assert_eq!(lib.index_of(&c), None);
    }

    #[test]
    fn switching_does_not_carry_a_grade_to_the_next_photograph() {
        let mut library = Library::new(
            vec![
                PathBuf::from("Z:/none/a.jpg"),
                PathBuf::from("Z:/none/b.jpg"),
            ],
            Support::default(),
        );
        let (mut history, ids) = library.take_current(None);
        let id = history
            .document()
            .stack
            .find_by_effect("exposure")
            .expect("a pinned row");
        history.edit("push", None, |doc| {
            if let Some(row) = doc.stack.get_mut(id) {
                row.params.set("ev", pe_core::ParamValue::Float(2.0));
            }
        });

        let (next, _) = library.switch(1, history, ids, None);
        let carried = next
            .document()
            .stack
            .get(id)
            .and_then(|r| r.params.get("ev"))
            .and_then(pe_core::ParamValue::as_float)
            .expect("set");
        assert_eq!(carried, 0.0, "the first photograph's exposure came along");
    }

    /// And going back finds it again, which is the other half of the same
    /// promise — parking is not discarding.
    #[test]
    fn switching_back_returns_the_edit_that_was_parked() {
        let mut library = Library::new(
            vec![
                PathBuf::from("Z:/none/a.jpg"),
                PathBuf::from("Z:/none/b.jpg"),
            ],
            Support::default(),
        );
        let (mut history, ids) = library.take_current(None);
        let id = history
            .document()
            .stack
            .find_by_effect("exposure")
            .expect("a pinned row");
        history.edit("push", None, |doc| {
            if let Some(row) = doc.stack.get_mut(id) {
                row.params.set("ev", pe_core::ParamValue::Float(2.0));
            }
        });

        let (b, b_ids) = library.switch(1, history, ids, None);
        let (a, _) = library.switch(0, b, b_ids, None);
        let back = a
            .document()
            .stack
            .get(id)
            .and_then(|r| r.params.get("ev"))
            .and_then(pe_core::ParamValue::as_float)
            .expect("set");
        assert_eq!(back, 2.0, "the parked edit was lost");
    }
    use super::*;

    fn library(names: &[&str]) -> Library {
        Library::new(
            names.iter().map(PathBuf::from).collect(),
            Support::default(),
        )
    }

    #[test]
    fn adding_the_same_photo_twice_does_not_duplicate_it() {
        let mut lib = library(&["a.jpg", "b.jpg"]);
        assert_eq!(lib.add([PathBuf::from("a.jpg")]), None);
        assert_eq!(lib.len(), 2);
        assert_eq!(lib.add([PathBuf::from("c.jpg")]), Some(2));
        assert_eq!(lib.len(), 3);
    }

    /// Opening files and then landing on none of them would be a strange thing
    /// for a program to do.
    #[test]
    fn adding_reports_the_first_new_photo_to_select() {
        let mut lib = library(&["a.jpg"]);
        let first = lib.add([PathBuf::from("a.jpg"), PathBuf::from("b.jpg")]);
        assert_eq!(first, Some(1), "the duplicate should not have been chosen");
    }

    /// Switching away and back must not throw away an undo stack. Losing an
    /// hour of work by clicking the wrong thumbnail is not a tolerable way for
    /// an editor to behave.
    #[test]
    fn an_edit_survives_switching_away_and_back() {
        let mut lib = library(&["a.jpg", "b.jpg"]);
        let (mut history, ids) = lib.switch(
            0,
            History::new(Document::from_path("a.jpg")),
            RowIdGenerator::default(),
            None,
        );
        history.edit("Exposure", None, |doc| {
            doc.stack
                .push(pe_core::StackRow::new(pe_core::RowId(1), "exposure"))
        });
        assert!(history.can_undo());

        let (other, other_ids) = lib.switch(1, history, ids, None);
        assert!(!other.can_undo(), "photo b arrived with a stack from a");

        let (back, _) = lib.switch(0, other, other_ids, None);
        assert!(back.can_undo(), "photo a's history was lost");
        assert_eq!(back.document().stack.len(), 1);
    }

    #[test]
    fn a_photo_that_has_never_been_opened_is_not_marked_edited() {
        let lib = library(&["a.jpg"]);
        assert!(!lib.entries()[0].edited());
    }

    #[test]
    fn a_photo_opened_and_left_alone_is_not_marked_edited() {
        let mut lib = library(&["a.jpg", "b.jpg"]);
        let (h, ids) = lib.switch(
            0,
            History::new(Document::from_path("a.jpg")),
            RowIdGenerator::default(),
            None,
        );
        let (h, ids) = lib.switch(1, h, ids, None);
        let _ = lib.switch(0, h, ids, None);
        assert!(
            !lib.entries()[1].edited(),
            "merely visiting a photo is not editing it"
        );
    }

    /// The case that was actually broken: an edit saved beside a photograph
    /// nobody has opened this session.
    ///
    /// Paste to all used to hand those a blank document, so a crop saved in a
    /// sidecar was thrown away by an action that promises to leave framing
    /// alone. Whether the promise held depended on whether you had happened to
    /// click on the photograph — the worst kind of inconsistency, because the
    /// thing that changes the behaviour leaves no trace.
    ///
    /// Through a real file on disc, because the whole bug lives in the branch
    /// that reads one.
    #[test]
    fn a_pasted_grade_leaves_a_saved_crop_alone() {
        let dir = std::env::temp_dir().join("kroma-paste-test");
        std::fs::create_dir_all(&dir).unwrap();
        let photo = dir.join("never-opened.jpg");
        let sidecar = photo.with_extension("peproj");

        let mut saved = pe_effects::new_document(photo.to_string_lossy().to_string());
        saved.geometry.size = [0.4, 0.4];
        std::fs::write(&sidecar, saved.to_json().unwrap()).unwrap();
        // Nothing in the autosave store for it, so the sidecar is what is read.
        // Guaranteed by the `Support::default()` the library below is built
        // with, which has nowhere to read from — this used to be arranged by
        // deleting the entry out of the developer's own autosave directory,
        // which is not a thing a test should be reaching into.

        let mut lib = Library::new(
            vec![PathBuf::from("in-hand.jpg"), photo.clone()],
            Support::default(),
        );
        let mut stack = pe_core::Stack::default();
        stack.push(pe_core::StackRow::new(pe_core::RowId(7), "grain"));
        assert_eq!(lib.paste_stack_to_all(&stack), 1);

        let after = lib.entries()[1].document().expect("pasted");
        assert_eq!(after.stack.len(), 1, "the grade did not arrive");
        assert_eq!(
            after.geometry.size,
            [0.4, 0.4],
            "a crop saved beside the photograph was thrown away by a paste"
        );
        let _ = std::fs::remove_file(&sidecar);
    }

    /// The grade travels; the framing stays where it was.
    ///
    /// The module says so, and it was only true for photographs that happened
    /// to have been opened this session — the rest had their whole document
    /// replaced, crop and all. This pins the promise for the case that used to
    /// break it: an entry whose edit is sitting parked rather than in hand.
    #[test]
    fn a_pasted_grade_leaves_the_crop_alone() {
        let mut lib = library(&["a.jpg", "b.jpg"]);

        // Visit b, crop it, and come back — which parks b's edit the way the
        // application does rather than reaching into the entry.
        let (mut b, b_ids) = lib.switch(
            1,
            History::new(Document::from_path("a.jpg")),
            RowIdGenerator::default(),
            None,
        );
        b.edit("Crop", None, |doc| doc.geometry.size = [0.5, 0.5]);
        let (a, a_ids) = lib.switch(0, b, b_ids, None);
        let _ = (a, a_ids);

        let mut stack = pe_core::Stack::default();
        stack.push(pe_core::StackRow::new(pe_core::RowId(7), "grain"));
        lib.paste_stack_to_all(&stack);

        let after = lib.entries()[1].document().expect("pasted");
        assert_eq!(after.stack.len(), 1, "the grade did not arrive");
        assert_eq!(
            after.geometry.size,
            [0.5, 0.5],
            "the crop was replaced along with the grade"
        );
    }

    #[test]
    fn pasting_a_grade_reaches_every_other_photo() {
        let mut lib = library(&["a.jpg", "b.jpg", "c.jpg"]);
        let mut stack = pe_core::Stack::default();
        stack.push(pe_core::StackRow::new(pe_core::RowId(7), "grain"));

        assert_eq!(lib.paste_stack_to_all(&stack), 2);
        assert!(
            lib.entries()[0].document().is_none(),
            "the photo in hand should be left to the caller"
        );
        for i in 1..3 {
            let doc = lib.entries()[i].document().expect("pasted");
            assert_eq!(doc.stack.len(), 1, "photo {i} did not receive the grade");
        }
    }

    /// A paste is an edit like any other, and an editor that cannot undo one
    /// applied to fifty photographs at once is worse than one that cannot
    /// apply it.
    #[test]
    fn a_pasted_grade_can_be_undone() {
        let mut lib = library(&["a.jpg", "b.jpg"]);
        let mut stack = pe_core::Stack::default();
        stack.push(pe_core::StackRow::new(pe_core::RowId(1), "grain"));
        lib.paste_stack_to_all(&stack);

        let (mut history, _) = lib.switch(
            1,
            History::new(Document::from_path("a.jpg")),
            RowIdGenerator::default(),
            None,
        );
        assert!(history.can_undo());
        history.undo();
        // Back to the document the photo would have had if nobody had pasted
        // anything: the pinned rows and nothing else.
        assert_eq!(
            history.document().stack.len(),
            pe_effects::PINNED_ROWS.len(),
            "undoing a paste did not restore the untouched document"
        );
    }

    #[test]
    fn removing_a_photo_before_the_current_one_keeps_the_selection() {
        let mut lib = library(&["a.jpg", "b.jpg", "c.jpg"]);
        let (h, ids) = lib.switch(
            2,
            History::new(Document::from_path("a.jpg")),
            RowIdGenerator::default(),
            None,
        );
        let _ = (h, ids);
        assert_eq!(lib.current(), 2);
        lib.remove(0);
        assert_eq!(lib.len(), 2);
        assert_eq!(
            lib.path(lib.current()).unwrap(),
            Path::new("c.jpg"),
            "the selection followed the wrong photo"
        );
    }

    #[test]
    fn removing_the_last_photo_leaves_the_selection_in_range() {
        let mut lib = library(&["a.jpg", "b.jpg"]);
        let (h, ids) = lib.switch(
            1,
            History::new(Document::from_path("a.jpg")),
            RowIdGenerator::default(),
            None,
        );
        let _ = (h, ids);
        lib.remove(1);
        assert_eq!(lib.current(), 0);
    }
}
