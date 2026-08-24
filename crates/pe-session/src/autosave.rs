//! Work in progress, kept so that closing the window is not a decision.
//!
//! Every edit is written to a store beside the application's settings within
//! a second of you stopping, and read back when the photograph is next opened.
//! The point is that pausing costs nothing: close the window mid-grade, come
//! back tomorrow, carry on.
//!
//! **Not beside the photograph.** A `.peproj` sidecar is something you ask
//! for; this happens whether you asked or not, and an application that
//! sprinkles files through somebody's photo library without being told to is
//! doing something it was not invited to do. It is kept **with the
//! application**, in the support directory the host names —
//! `%APPDATA%\Kroma` on Windows, `~/Library/Application Support/Kroma` on a
//! Mac, the app container on iOS — and not beside your photographs. A
//! photo directory that has never been written to stays that way.
//!
//! The two are not rivals. A sidecar is a decision — *this* is the edit, keep
//! it, move it with the photograph, put it under version control. The autosave
//! is just where you happened to stop.

use std::path::{Path, PathBuf};

use pe_core::Document;
use serde::{Deserialize, Serialize};

use crate::Support;

/// What one file holds.
///
/// The source path is written into the file as well as hashed into its name,
/// so a hash collision is something that can be *detected* rather than
/// something that silently hands you another photograph's grade. Two paths
/// colliding is unlikely; two paths colliding and being read as each other's
/// edits is unacceptable, and the difference between those costs one string.
#[derive(Serialize, Deserialize)]
struct Entry {
    source: String,
    document: Document,
}

/// A stable name for a photograph's edit file.
///
/// FNV-1a, written out here rather than borrowed from the standard library:
/// `DefaultHasher` is explicitly not stable between releases, and a hash that
/// changes when the toolchain does would quietly orphan everybody's work in
/// progress. This one is fixed forever by being sixteen lines long.
fn key(photo: &Path) -> String {
    let canonical = std::fs::canonicalize(photo).unwrap_or_else(|_| photo.to_path_buf());
    let text = canonical.to_string_lossy().to_lowercase();
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in text.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100_0000_01b3);
    }
    format!("{hash:016x}.json")
}

fn path_for(support: &Support, photo: &Path) -> Option<PathBuf> {
    Some(support.edits_dir()?.join(key(photo)))
}

/// What was being worked on, if anything.
///
/// Every failure means the same thing — there is nothing saved for this
/// photograph — and none of them is worth interrupting an open over. A store
/// that cannot be read costs the user their work in progress, which is bad;
/// refusing to show them the photograph as well would be worse.
pub fn load(support: &Support, photo: &Path) -> Option<Document> {
    let text = std::fs::read_to_string(path_for(support, photo)?).ok()?;
    let entry: Entry = serde_json::from_str(&text).ok()?;
    // The collision check. A file whose recorded source is not this
    // photograph belongs to another one.
    let canonical = std::fs::canonicalize(photo).unwrap_or_else(|_| photo.to_path_buf());
    (entry
        .source
        .eq_ignore_ascii_case(&canonical.to_string_lossy()))
    .then_some(entry.document)
}

/// Write the work in progress out.
///
/// Returns what went wrong rather than swallowing it. This used to discard the
/// result, on the reasoning that a failed autosave is not worth interrupting
/// anybody over — which is true, and is an argument about how loudly to say
/// so, not about whether to find out. It hid a bug that stopped autosave
/// working entirely on every network volume: silent success and silent failure
/// look identical from here, and only one of them is acceptable.
///
/// A host that does not care can still ignore the result. The difference is
/// that now it is choosing to.
pub fn store(support: &Support, photo: &Path, document: &Document) -> Result<(), Error> {
    let Some(path) = path_for(support, photo) else {
        // No support directory means the host has not said where to write, so
        // writing nothing is the agreed outcome rather than a failure.
        return Ok(());
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| Error::Write(e.to_string()))?;
    }
    let canonical = std::fs::canonicalize(photo).unwrap_or_else(|_| photo.to_path_buf());
    let entry = Entry {
        source: canonical.to_string_lossy().to_string(),
        document: document.clone(),
    };
    let json = serde_json::to_string(&entry).map_err(|e| Error::Encode(e.to_string()))?;
    // The one that matters most. This is rewritten every time the user stops
    // moving, it is the only copy of work nobody asked to save, and `load`
    // treats an unparseable file as nothing saved at all — so a torn write here
    // loses the lot.
    pe_io::write_bytes_atomically(path, json.as_bytes()).map_err(|e| Error::Write(e.to_string()))
}

/// Why the work in progress could not be written.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("could not write the autosave: {0}")]
    Write(String),
    #[error("could not encode the autosave: {0}")]
    Encode(String),
}

/// Throw the saved work away.
///
/// The counterpart to saving without being asked. An edit that comes back
/// every time you open a photograph, with no way to be rid of it, is not a
/// convenience — it is a photograph you can no longer see.
pub fn forget(support: &Support, photo: &Path) {
    if let Some(path) = path_for(support, photo) {
        let _ = std::fs::remove_file(path);
    }
}

/// How long a pause counts as "stopped".
///
/// Long enough that a slider drag is one write rather than sixty, short enough
/// that the gap between putting the mouse down and closing the window is not
/// somewhere work can be lost.
pub const IDLE: std::time::Duration = std::time::Duration::from_millis(900);

/// Watches the revision counter and decides when to write.
///
/// Separate from the writing so the decision can be tested without a disc: it
/// is the part with the throttle in it, and a throttle is exactly the sort of
/// thing that works in the obvious case and not at the edges.
pub struct Watcher {
    seen: u64,
    written: u64,
    changed_at: Option<std::time::Instant>,
}

impl Default for Watcher {
    fn default() -> Self {
        Self::new()
    }
}

impl Watcher {
    pub fn new() -> Self {
        Self {
            seen: 0,
            written: 0,
            changed_at: None,
        }
    }

    /// Called every frame. True when it is time to write.
    pub fn tick(&mut self, revision: u64, now: std::time::Instant) -> bool {
        if revision != self.seen {
            self.seen = revision;
            self.changed_at = Some(now);
            return false;
        }
        // Unsaved work, and the user has stopped moving.
        if self.seen != self.written
            && let Some(at) = self.changed_at
            && now.duration_since(at) >= IDLE
        {
            self.written = self.seen;
            return true;
        }
        false
    }

    /// Whether anything is waiting to be written.
    ///
    /// Asked when leaving a photograph, where the throttle is beside the
    /// point — the thing that would have triggered the write is about to stop
    /// being the thing on screen.
    pub fn pending(&self) -> bool {
        self.seen != self.written
    }

    /// Start again on a different photograph.
    pub fn reset(&mut self, revision: u64) {
        self.seen = revision;
        self.written = revision;
        self.changed_at = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn doc_with_note(note: &str) -> Document {
        let mut d = Document::from_path("photo.jpg");
        d.metadata.note = Some(note.to_string());
        d
    }

    #[test]
    fn an_edit_comes_back_from_the_store_it_was_put_in() {
        let tmp = tempfile::tempdir().unwrap();
        let support = Support::at(tmp.path());
        let photo = tmp.path().join("a.jpg");
        std::fs::write(&photo, b"not really a jpeg").unwrap();

        store(&support, &photo, &doc_with_note("in progress"))
            .expect("the temporary directory is writable");
        let back = load(&support, &photo).expect("stored, so it loads");
        assert_eq!(back.metadata.note.as_deref(), Some("in progress"));
    }

    #[test]
    fn nothing_is_written_when_the_host_never_said_where() {
        // The whole point of Support being an Option. A store with nowhere to
        // go writes nothing rather than choosing somewhere.
        let tmp = tempfile::tempdir().unwrap();
        let photo = tmp.path().join("a.jpg");
        std::fs::write(&photo, b"x").unwrap();

        store(&Support::default(), &photo, &doc_with_note("lost"))
            .expect("a store with nowhere to go is not a failure");
        assert!(load(&Support::default(), &photo).is_none());
        // And nothing appeared beside the photograph either.
        let beside: Vec<_> = std::fs::read_dir(tmp.path()).unwrap().collect();
        assert_eq!(beside.len(), 1, "something was written next to the photo");
    }

    #[test]
    fn one_photographs_edit_is_never_handed_to_another() {
        // The recorded source is the collision check. Two paths hashing to one
        // name is unlikely; being read as each other's edits is unacceptable.
        let tmp = tempfile::tempdir().unwrap();
        let support = Support::at(tmp.path());
        let a = tmp.path().join("a.jpg");
        let b = tmp.path().join("b.jpg");
        std::fs::write(&a, b"x").unwrap();
        std::fs::write(&b, b"y").unwrap();

        store(&support, &a, &doc_with_note("belongs to a"))
            .expect("the temporary directory is writable");
        assert!(load(&support, &b).is_none());
    }

    #[test]
    fn forgetting_leaves_nothing_to_come_back() {
        let tmp = tempfile::tempdir().unwrap();
        let support = Support::at(tmp.path());
        let photo = tmp.path().join("a.jpg");
        std::fs::write(&photo, b"x").unwrap();

        store(&support, &photo, &doc_with_note("temporary"))
            .expect("the temporary directory is writable");
        forget(&support, &photo);
        assert!(load(&support, &photo).is_none());
    }

    #[test]
    fn nothing_is_written_while_the_value_is_still_moving() {
        let mut w = Watcher::new();
        let start = Instant::now();
        for i in 1..40u64 {
            let at = start + Duration::from_millis(i * 16);
            assert!(!w.tick(i, at), "wrote during a drag, at frame {i}");
        }
    }

    #[test]
    fn a_pause_after_a_change_writes_once() {
        let mut w = Watcher::new();
        let start = Instant::now();
        assert!(!w.tick(1, start));
        assert!(!w.tick(1, start + IDLE / 2));
        assert!(w.tick(1, start + IDLE), "did not write after the pause");
        assert!(
            !w.tick(1, start + IDLE * 3),
            "wrote a second time with nothing new"
        );
    }

    #[test]
    fn leaving_a_photograph_knows_there_is_work_outstanding() {
        let mut w = Watcher::new();
        assert!(!w.pending());
        w.tick(1, Instant::now());
        assert!(w.pending());
        w.reset(1);
        assert!(!w.pending());
    }
}
