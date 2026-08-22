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
//! doing something it was not invited to do. It lives under `%APPDATA%\Kroma`
//! with the settings, and a photo directory that has never been written to
//! stays that way.
//!
//! The two are not rivals. A sidecar is a decision — *this* is the edit, keep
//! it, move it with the photograph, put it under version control. The autosave
//! is just where you happened to stop.

use std::path::{Path, PathBuf};

use pe_core::Document;
use serde::{Deserialize, Serialize};

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

fn dir() -> Option<PathBuf> {
    let base = if cfg!(windows) {
        PathBuf::from(std::env::var_os("APPDATA")?)
    } else {
        match std::env::var_os("XDG_CONFIG_HOME") {
            Some(v) => PathBuf::from(v),
            None => PathBuf::from(std::env::var_os("HOME")?).join(".config"),
        }
    };
    Some(base.join("Kroma").join("edits"))
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

fn path_for(photo: &Path) -> Option<PathBuf> {
    Some(dir()?.join(key(photo)))
}

/// What was being worked on, if anything.
///
/// Every failure means the same thing — there is nothing saved for this
/// photograph — and none of them is worth interrupting an open over. A store
/// that cannot be read costs the user their work in progress, which is bad;
/// refusing to show them the photograph as well would be worse.
pub fn load(photo: &Path) -> Option<Document> {
    let text = std::fs::read_to_string(path_for(photo)?).ok()?;
    let entry: Entry = serde_json::from_str(&text).ok()?;
    // The collision check. A file whose recorded source is not this
    // photograph belongs to another one.
    let canonical = std::fs::canonicalize(photo).unwrap_or_else(|_| photo.to_path_buf());
    (entry
        .source
        .eq_ignore_ascii_case(&canonical.to_string_lossy()))
    .then_some(entry.document)
}

pub fn store(photo: &Path, document: &Document) {
    let Some(path) = path_for(photo) else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let canonical = std::fs::canonicalize(photo).unwrap_or_else(|_| photo.to_path_buf());
    let entry = Entry {
        source: canonical.to_string_lossy().to_string(),
        document: document.clone(),
    };
    if let Ok(json) = serde_json::to_string(&entry) {
        let _ = std::fs::write(path, json);
    }
}

/// Throw the saved work away.
///
/// The counterpart to saving without being asked. An edit that comes back
/// every time you open a photograph, with no way to be rid of it, is not a
/// convenience — it is a photograph you can no longer see.
pub fn forget(photo: &Path) {
    if let Some(path) = path_for(photo) {
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

    #[test]
    fn nothing_is_written_while_the_value_is_still_moving() {
        let mut w = Watcher::new();
        let start = Instant::now();
        // A drag: the revision moves every frame.
        for i in 1..40u64 {
            let at = start + Duration::from_millis(i * 16);
            assert!(!w.tick(i, at), "wrote during a drag, at frame {i}");
        }
        assert!(w.pending(), "the drag left nothing to write");
    }

    #[test]
    fn a_pause_writes_once_and_then_stops() {
        let mut w = Watcher::new();
        let start = Instant::now();
        w.tick(1, start);
        assert!(!w.tick(1, start + Duration::from_millis(100)));
        assert!(w.tick(1, start + IDLE + Duration::from_millis(1)));
        // And does not keep writing the same thing.
        assert!(!w.tick(1, start + IDLE + Duration::from_secs(10)));
        assert!(!w.pending());
    }

    #[test]
    fn a_second_change_after_a_write_is_written_too() {
        let mut w = Watcher::new();
        let start = Instant::now();
        w.tick(1, start);
        assert!(w.tick(1, start + IDLE * 2));
        w.tick(2, start + IDLE * 3);
        assert!(w.tick(2, start + IDLE * 5));
    }

    /// The store itself, through a real file.
    ///
    /// The watcher's tests are about *when*; this is about *what*, and it is
    /// worth doing on disc rather than in memory because the parts that can go
    /// wrong — the hashed name, the recorded source, the collision check — all
    /// live in the round trip.
    #[test]
    fn an_edit_comes_back_for_the_photograph_it_was_made_on() {
        let Some(dir) = dir() else {
            return;
        };
        // A path that will not exist, which is also the interesting case:
        // canonicalize fails and the raw path has to carry the identity.
        let photo = dir.join("a-photograph-that-is-not-there.jpg");
        let other = dir.join("a-different-one.jpg");
        forget(&photo);
        forget(&other);
        assert!(load(&photo).is_none(), "started with something saved");

        let mut doc = Document::from_path(photo.to_string_lossy().to_string());
        doc.stack.rows.clear();
        store(&photo, &doc);

        let back = load(&photo).expect("the edit did not come back");
        assert!(back.stack.rows.is_empty());
        // And it belongs to that photograph alone.
        assert!(load(&other).is_none(), "another photograph read this edit");

        forget(&photo);
        assert!(load(&photo).is_none(), "forgetting left it behind");
    }

    /// Switching photographs must not carry the outgoing one's pending state
    /// onto the incoming one, or the first pause after a switch writes the
    /// wrong document — or nothing at all.
    #[test]
    fn resetting_clears_what_was_pending() {
        let mut w = Watcher::new();
        w.tick(7, Instant::now());
        assert!(w.pending());
        w.reset(3);
        assert!(!w.pending());
    }
}
