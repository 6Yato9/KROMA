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
}

impl Library {
    pub fn new(paths: Vec<PathBuf>) -> Self {
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
    pub fn switch(
        &mut self,
        index: usize,
        outgoing: History,
        outgoing_ids: RowIdGenerator,
    ) -> (History, RowIdGenerator) {
        if let Some(entry) = self.entries.get_mut(self.current) {
            entry.parked = Some((outgoing, outgoing_ids));
        }
        self.current = index.min(self.entries.len().saturating_sub(1));
        self.take_current()
    }

    /// Take the edit belonging to the photograph currently pointed at,
    /// parking nothing.
    ///
    /// Separate from [`Library::switch`] for the one case where there is
    /// nothing to park: the edit in hand belonged to a photograph that has
    /// just been taken out of the set.
    pub fn take_current(&mut self) -> (History, RowIdGenerator) {
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
                let name = entry.path.to_string_lossy().to_string();
                let doc = load_edit(&entry.path).unwrap_or_else(|| pe_effects::new_document(name));
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
        let mut n = 0;
        for (i, entry) in self.entries.iter_mut().enumerate() {
            if i == current {
                continue;
            }
            let path = entry.path.to_string_lossy().to_string();
            let (history, ids) = entry.parked.take().unwrap_or_else(|| {
                let doc = pe_effects::new_document(path);
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
pub fn load_edit(path: &Path) -> Option<pe_core::Document> {
    let json = std::fs::read_to_string(path.with_extension("peproj")).ok()?;
    pe_core::Document::from_json(&json).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn library(names: &[&str]) -> Library {
        Library::new(names.iter().map(PathBuf::from).collect())
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
        );
        history.edit("Exposure", None, |doc| {
            doc.stack
                .push(pe_core::StackRow::new(pe_core::RowId(1), "exposure"))
        });
        assert!(history.can_undo());

        let (other, other_ids) = lib.switch(1, history, ids);
        assert!(!other.can_undo(), "photo b arrived with a stack from a");

        let (back, _) = lib.switch(0, other, other_ids);
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
        );
        let (h, ids) = lib.switch(1, h, ids);
        let _ = lib.switch(0, h, ids);
        assert!(
            !lib.entries()[1].edited(),
            "merely visiting a photo is not editing it"
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
        );
        let _ = (h, ids);
        lib.remove(1);
        assert_eq!(lib.current(), 0);
    }
}
