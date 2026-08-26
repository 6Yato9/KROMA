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
//!
//! Here rather than in a shell because all of it — the set, the parked edits,
//! and `switch` above all — is the same question in both, and a rule
//! implemented twice is a rule that will differ. The only part that could not
//! travel is the thumbnail: it arrives as bytes, and each shell turns those
//! into a texture belonging to its own graphics context.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, channel};

use pe_core::{Document, History, RowIdGenerator};

use crate::{Support, autosave};

/// Long edge of a filmstrip thumbnail, in pixels.
///
/// Big enough to recognise a frame at a glance and to survive a high-DPI
/// display; small enough that a thousand of them is 200 MB rather than 20 GB.
pub const THUMB_EDGE: u32 = 128;

/// The extensions the open dialogs offer and a folder scan accepts.
pub const EXTENSIONS: [&str; 3] = ["jpg", "jpeg", "png"];

/// A decoded thumbnail: RGBA, eight bits a channel, rows top to bottom.
///
/// Deliberately not any shell's image type. Both of them have one, they are
/// different types, and the day this crate names either is the day the other
/// shell needs a conversion out of a foreign format to get its own pixels.
#[derive(Clone, PartialEq, Eq)]
pub struct Thumbnail {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl std::fmt::Debug for Thumbnail {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never print sixty kilobytes of pixels into a test failure message.
        f.debug_struct("Thumbnail")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("bytes", &self.rgba.len())
            .finish()
    }
}

impl From<pe_io::DecodedImage> for Thumbnail {
    fn from(img: pe_io::DecodedImage) -> Self {
        Self {
            width: img.width,
            height: img.height,
            rgba: img.pixels,
        }
    }
}

/// One photograph in the set.
pub struct Entry {
    pub path: PathBuf,
    /// The edit, parked while a different photograph is being worked on.
    ///
    /// The whole history rather than just the document, so that switching away
    /// and back does not quietly throw away an undo stack. `None` means this
    /// photo has never been opened and gets a fresh document when it is.
    parked: Option<(History, RowIdGenerator)>,
    /// The thumbnail as RGBA bytes, `THUMB_EDGE` on its longest side.
    ///
    /// Bytes rather than a texture: a texture belongs to a graphics context
    /// and there are two shells with two of those. Each uploads its own.
    pub thumb: Option<Thumbnail>,
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

/// What the worker sends back. Raw pixels, not a texture: a texture belongs to
/// a graphics context, and this crate is not allowed to know about either of
/// the two the shells have.
struct Decoded {
    index: usize,
    image: Option<Thumbnail>,
}

/// How a job reaches whoever is decoding.
///
/// A closure rather than the worker's channel written into `Library`, so that
/// where the decoding happens is one line in a constructor instead of a
/// condition threaded through `request`. Production has exactly one of these:
/// hand the path to the thread spawned in [`Library::new`].
type Submit = Box<dyn FnMut(usize, &Path) + Send>;

pub struct Library {
    entries: Vec<Entry>,
    current: usize,
    submit: Submit,
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
                        .map(|img| Thumbnail::from(pe_io::thumbnail(&img, THUMB_EDGE)));
                    // A closed channel means the window has gone; there is
                    // nothing left to deliver to.
                    if tx.send(Decoded { index, image }).is_err() {
                        break;
                    }
                }
            })
            .expect("spawn thumbnail worker");

        Self::from_parts(
            paths,
            support,
            done,
            Box::new(move |index, path| {
                let _ = jobs.send((index, path.to_path_buf()));
            }),
        )
    }

    /// A library that decodes with `decode`, on the thread that calls
    /// [`Library::request`].
    ///
    /// For tests, and not out of tidiness. The worker is a real thread, so
    /// "ask for a thumbnail and then look at it" is otherwise a question about
    /// the scheduler: a test patient enough to be certain is slow for
    /// everybody, and one that is not is fine until it runs on a loaded
    /// machine. Handing the decode in runs the same path — request, channel,
    /// collect — to completion inside the call, so what is asserted is this
    /// module's behaviour and nothing else's.
    ///
    /// It moves where the work happens and nothing else: `request` still
    /// refuses to ask twice and `collect` still takes delivery over the
    /// channel, which is why the thread the shells actually run is worth one
    /// end-to-end test of its own rather than none.
    #[cfg(test)]
    fn with_decoder(
        paths: Vec<PathBuf>,
        support: Support,
        decode: impl Fn(&Path) -> Option<Thumbnail> + Send + 'static,
    ) -> Self {
        let (tx, done) = channel::<Decoded>();
        Self::from_parts(
            paths,
            support,
            done,
            Box::new(move |index, path| {
                let image = decode(path);
                let _ = tx.send(Decoded { index, image });
            }),
        )
    }

    fn from_parts(
        paths: Vec<PathBuf>,
        support: Support,
        done: Receiver<Decoded>,
        submit: Submit,
    ) -> Self {
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
            submit,
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
            (self.submit)(i, &entry.path);
        }
    }

    /// Take delivery of whatever the worker has finished. Returns true if
    /// anything arrived, so the caller knows to repaint — and, in a shell with
    /// a graphics context, to upload what turned up.
    pub fn collect(&mut self) -> bool {
        let mut any = false;
        while let Ok(decoded) = self.done.try_recv() {
            any = true;
            let Some(entry) = self.entries.get_mut(decoded.index) else {
                continue;
            };
            match decoded.image {
                Some(thumb) => entry.thumb = Some(thumb),
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
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

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

    /// Startup hands `focus` whatever index the last session ended on, and the
    /// folder may have lost photographs since it was written down.
    #[test]
    fn focusing_past_the_end_of_the_set_is_ignored() {
        let mut lib = library(&["a.jpg", "b.jpg"]);
        lib.focus(1);
        lib.focus(7);
        assert_eq!(
            lib.current(),
            1,
            "the selection went off the end of the set"
        );
    }

    #[test]
    fn removing_something_that_is_not_in_the_set_changes_nothing() {
        let mut lib = library(&["a.jpg", "b.jpg"]);
        lib.remove(9);
        assert_eq!(lib.len(), 2);
        assert_eq!(lib.current(), 0);
    }

    /// Taking out the photograph being worked on leaves the one that slid into
    /// its place selected — the next photograph in the set, which is where
    /// somebody working through a folder was heading anyway.
    #[test]
    fn removing_the_current_photograph_leaves_a_sensible_one_current() {
        let mut lib = library(&["a.jpg", "b.jpg", "c.jpg"]);
        let (h, ids) = lib.switch(
            1,
            History::new(Document::from_path("a.jpg")),
            RowIdGenerator::default(),
            None,
        );
        let _ = (h, ids);
        assert_eq!(lib.current(), 1);

        lib.remove(1);
        assert_eq!(lib.len(), 2);
        assert_eq!(
            lib.path(lib.current()).unwrap(),
            Path::new("c.jpg"),
            "the selection did not land on the photograph that took its place"
        );
    }

    #[test]
    fn removing_the_last_photograph_leaves_an_empty_library() {
        let mut lib = library(&["a.jpg"]);
        lib.remove(0);
        assert!(lib.is_empty());
        assert_eq!(lib.len(), 0);
        assert!(lib.path(0).is_none());
        assert_eq!(
            lib.current(),
            0,
            "an empty set points at nothing rather than at one past the end"
        );
        // And asking an empty set for an edit is a blank document, not a panic.
        // The shell reaches here: taking out the last photograph goes through
        // `take_current` before it has noticed the set is empty.
        let (history, _) = lib.take_current(None);
        assert!(history.document().stack.is_empty());
    }

    /// A photograph never opened gets the edit saved beside it, which is the
    /// whole point of writing one.
    #[test]
    fn a_photograph_never_opened_takes_the_edit_saved_beside_it() {
        let tmp = tempfile::tempdir().unwrap();
        let photo = tmp.path().join("never-opened.jpg");
        std::fs::write(&photo, b"not really a jpeg").unwrap();
        let mut saved = pe_effects::new_document(photo.to_string_lossy().to_string());
        saved.geometry.size = [0.3, 0.6];
        std::fs::write(photo.with_extension("peproj"), saved.to_json().unwrap()).unwrap();

        // `Support::default()` has nowhere to keep an autosave, so the sidecar
        // is the only edit there is to find.
        let mut lib = Library::new(vec![photo], Support::default());
        let (history, _) = lib.take_current(None);
        assert_eq!(
            history.document().geometry.size,
            [0.3, 0.6],
            "the edit saved beside the photograph was ignored"
        );
        assert!(
            !history.can_undo(),
            "an edit read off disc is where the undo stack starts, not something already done"
        );
    }

    /// The autosave is by construction the *later* of the two, so it is what
    /// you were actually looking at.
    #[test]
    fn an_autosave_wins_over_the_sidecar_beside_the_photograph() {
        let tmp = tempfile::tempdir().unwrap();
        let support = Support::at(tmp.path().join("support"));
        let photo = tmp.path().join("both.jpg");
        std::fs::write(&photo, b"not really a jpeg").unwrap();

        let mut sidecar = pe_effects::new_document(photo.to_string_lossy().to_string());
        sidecar.geometry.size = [0.3, 0.6];
        std::fs::write(photo.with_extension("peproj"), sidecar.to_json().unwrap()).unwrap();

        let mut later = pe_effects::new_document(photo.to_string_lossy().to_string());
        later.geometry.size = [0.9, 0.9];
        autosave::store(&support, &photo, &later).expect("the temporary directory is writable");

        let mut lib = Library::new(vec![photo], support);
        let (history, _) = lib.take_current(None);
        assert_eq!(
            history.document().geometry.size,
            [0.9, 0.9],
            "the sidecar was read over work that had gone further"
        );
    }

    /// And a photograph with no edit anywhere gets a fresh document believing
    /// what the file said about itself — the one moment a declared space is
    /// applied.
    #[test]
    fn a_photograph_with_no_edit_anywhere_takes_the_space_the_file_declared() {
        let mut lib = library(&["a.jpg"]);
        let (history, _) = lib.take_current(Some("Display P3"));
        assert_eq!(history.document().color.input, "Display P3");
    }

    #[test]
    fn a_folder_scan_takes_only_the_extensions_the_dialogs_offer() {
        let tmp = tempfile::tempdir().unwrap();
        for name in [
            "b.jpg",
            "a.PNG",
            "c.jpeg",
            "notes.txt",
            "d.tiff",
            "e.peproj",
            "no-extension",
        ] {
            std::fs::write(tmp.path().join(name), b"x").unwrap();
        }

        let names: Vec<String> = Library::scan(tmp.path())
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().to_string())
            .collect();
        // In name order, because a directory listing arrives in whatever order
        // the filesystem felt like and a strip that shuffles between runs is
        // unusable. And `a.PNG` is in it: the extension is matched folded, or
        // half of everybody's camera output is invisible.
        assert_eq!(names, ["a.PNG", "b.jpg", "c.jpeg"]);
    }

    /// A folder that is not there is empty rather than a panic. The session
    /// being reopened names one that may since have been moved or unplugged.
    #[test]
    fn a_scan_of_somewhere_that_is_not_there_finds_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(Library::scan(&tmp.path().join("gone")).is_empty());
    }

    /// Stand-in pixels, recognisable by their size.
    fn pretend(width: u32, height: u32) -> Thumbnail {
        Thumbnail {
            width,
            height,
            rgba: vec![255; (width * height * 4) as usize],
        }
    }

    /// A library that decodes on the calling thread, counting how often it was
    /// asked to. "How many times" is the whole question in more than one test
    /// below, and a thread cannot answer it without being waited on.
    fn counted(names: &[&str]) -> (Library, Arc<AtomicUsize>) {
        let calls = Arc::new(AtomicUsize::new(0));
        let seen = Arc::clone(&calls);
        let lib = Library::with_decoder(
            names.iter().map(PathBuf::from).collect(),
            Support::default(),
            move |_| {
                seen.fetch_add(1, Ordering::SeqCst);
                Some(pretend(8, 6))
            },
        );
        (lib, calls)
    }

    #[test]
    fn a_library_that_asked_for_nothing_collects_nothing() {
        let mut lib = library(&["a.jpg"]);
        assert!(!lib.collect(), "something arrived that was never asked for");
        assert!(lib.entries()[0].thumb.is_none());
    }

    #[test]
    fn a_requested_thumbnail_arrives_as_bytes() {
        let (mut lib, _) = counted(&["a.jpg", "b.jpg"]);
        lib.request(0..1);
        assert!(lib.collect(), "collect did not report the delivery");

        let thumb = lib.entries()[0].thumb.as_ref().expect("a thumbnail");
        assert_eq!((thumb.width, thumb.height), (8, 6));
        assert_eq!(thumb.rgba.len(), 8 * 6 * 4, "not four bytes to the pixel");
        assert!(
            lib.entries()[1].thumb.is_none(),
            "b was decoded without being asked for"
        );
    }

    /// Asking twice does not decode twice.
    #[test]
    fn a_thumbnail_is_only_requested_once() {
        let (mut lib, decodes) = counted(&["a.jpg"]);
        // Twice before anything is collected — which is what a strip redrawing
        // at sixty frames a second does while the first decode is still in
        // flight.
        lib.request(0..1);
        lib.request(0..1);
        lib.collect();
        // And again once it has arrived.
        lib.request(0..1);
        lib.collect();
        assert_eq!(
            decodes.load(Ordering::SeqCst),
            1,
            "the same photograph was decoded more than once"
        );
    }

    #[test]
    fn only_the_range_asked_for_is_decoded() {
        let (mut lib, decodes) = counted(&["a.jpg", "b.jpg", "c.jpg", "d.jpg"]);
        lib.request(1..3);
        lib.collect();
        assert_eq!(decodes.load(Ordering::SeqCst), 2);
        assert!(lib.entries()[0].thumb.is_none());
        assert!(lib.entries()[3].thumb.is_none());
    }

    /// The strip asks for a lookahead past the last cell on screen, which on a
    /// short set runs off the end of it. That is the caller being
    /// straightforward rather than careless, and it is this end that copes.
    #[test]
    fn a_request_running_past_the_end_asks_for_what_is_there() {
        let (mut lib, decodes) = counted(&["a.jpg", "b.jpg", "c.jpg"]);
        lib.request(0..1000);
        assert_eq!(decodes.load(Ordering::SeqCst), 3);
    }

    /// A file that will not decode is marked and left alone, with no
    /// thumbnail: the strip draws "unreadable" rather than a spinner that
    /// never stops.
    #[test]
    fn a_photograph_that_will_not_decode_is_marked_unreadable() {
        let mut lib = Library::with_decoder(
            vec![PathBuf::from("shredded.jpg")],
            Support::default(),
            |_| None,
        );
        lib.request(0..1);
        assert!(lib.collect(), "the failure never came back");
        assert!(lib.entries()[0].failed);
        assert!(lib.entries()[0].thumb.is_none());
    }

    /// The worker the shells actually run, end to end: a real file on disc,
    /// decoded on a real thread, arriving over the channel.
    ///
    /// Everything above hands the decode in, which is what makes those tests
    /// about this module rather than about the scheduler — and would also let
    /// the thread rot without a single test noticing. This one polls `collect`
    /// to a deadline and says what failed to turn up, rather than sleeping for
    /// a guessed interval and hoping.
    #[test]
    fn a_thumbnail_comes_back_from_the_worker_thread() {
        let tmp = tempfile::tempdir().unwrap();
        let photo = tmp.path().join("chart.png");
        pe_io::save_png(&pe_io::test_chart(320, 240), &photo, &pe_color::space::SRGB)
            .expect("the temporary directory is writable");

        let mut lib = Library::new(vec![photo], Support::default());
        lib.request(0..1);

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            lib.collect();
            let entry = &lib.entries()[0];
            if entry.thumb.is_some() || entry.failed {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "the thumbnail worker delivered nothing in thirty seconds"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }

        let thumb = lib.entries()[0]
            .thumb
            .as_ref()
            .expect("the worker could not read a file it had just written");
        assert_eq!(
            thumb.width, THUMB_EDGE,
            "the long edge was not reduced to THUMB_EDGE"
        );
        assert_eq!(thumb.height, 96, "the aspect ratio did not survive");
        assert_eq!(thumb.rgba.len(), (thumb.width * thumb.height * 4) as usize);
    }
}
