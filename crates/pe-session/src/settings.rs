//! What the application remembers between runs.
//!
//! Not the edit — that is the document, and it lives beside the photograph.
//! This is the handful of things that belong to the *person* rather than to
//! any one picture: which effects they have starred, the set that was open,
//! how they export, and whatever joins that list later.
//!
//! A favourite that vanishes when the window closes is half a feature, which
//! is the whole reason this file exists. It is deliberately small and
//! deliberately forgiving — a settings file that fails to parse costs the user
//! their stars, not their session, so every error here is swallowed and the
//! defaults stand.
//!
//! Here rather than in a shell because none of it is a question about a
//! window. A star means the same thing in both shells, so does the set you
//! left open, and so does exporting JPEGs at 92 — and an answer that depends
//! on which shell you happened to open is an answer given twice. What stays in
//! a shell is per-shell interface state: which panel is folded, whether the
//! scopes are showing. Those genuinely are about the window.
//!
//! Where the file lives is the one thing this module does not decide.
//! [`Support`] is handed in by the host at start-up, for the reason written on
//! it: an engine that worked the directory out from environment variables
//! would be guessing on a platform nobody tested, and it was already wrong on
//! the Mac.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::Support;
use crate::export::Export;

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Effect keys the user has starred, in the order they starred them.
    pub favourites: Vec<String>,
    /// The photographs that were open when the window last closed, and which
    /// one was showing.
    ///
    /// Stored as strings rather than as `PathBuf` so the file stays readable
    /// and portable — a settings file is something a person may end up looking
    /// at, and a serialised platform path is not.
    #[serde(default)]
    session: Vec<String>,
    #[serde(default)]
    session_index: usize,
    /// How the last export was written. Remembered because it is a decision
    /// about the work rather than about one photograph — somebody exporting
    /// JPEGs at 92 is going to keep doing it, and asking again every time is
    /// asking them to answer a question they have already answered.
    #[serde(default)]
    pub export: Export,
    /// Anything a newer build wrote that this one does not know about, kept
    /// so that running an older version does not silently discard it.
    #[serde(flatten)]
    unknown: serde_json::Map<String, serde_json::Value>,
}

impl Settings {
    /// What was remembered, or the defaults.
    ///
    /// Every failure here is the same failure: there are no settings yet. A
    /// host that named no support directory, a missing file, an unreadable one
    /// and a corrupt one all mean the user gets the defaults, and none of them
    /// is worth interrupting a launch over — which is why this returns a
    /// `Settings` rather than a `Result` in a crate that returns `Result`
    /// almost everywhere else.
    pub fn load(support: &Support) -> Self {
        // The old location is read when the new one is not there yet, so
        // the rename to Kroma does not quietly cost anyone their stars. It
        // moves to the new path the next time anything is saved.
        let former = support
            .root()
            .and_then(|root| root.parent())
            .map(|d| d.join("PhotoEditor").join("settings.json"));
        support
            .settings_path()
            .into_iter()
            .chain(former)
            .filter_map(|p| std::fs::read_to_string(p).ok())
            .find_map(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    /// Write them down, if there is anywhere to write them.
    ///
    /// A host that has named no support directory has not agreed to anything
    /// being written, so nothing is — and, like everything else here, that is
    /// not an error anybody is told about.
    pub fn save(&self, support: &Support) {
        let Some(path) = support.settings_path() else {
            return;
        };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_string_pretty(self) {
            // Through a scratch file like everything else that is written
            // without being asked: settings are rewritten on the way out, which
            // is exactly when a process is most likely to be cut short.
            let _ = pe_io::write_bytes_atomically(path, json.as_bytes());
        }
    }

    /// What to reopen, and where in it to start.
    ///
    /// Filtered by what still exists. A photograph moved or deleted since the
    /// last run must not stop the application starting, and silently leaving
    /// it out is the only reasonable thing to do about it — there is nobody
    /// to tell yet.
    pub fn session(&self) -> (Vec<PathBuf>, usize) {
        // Which one was showing, by name rather than by position. Dropping the
        // photographs that have gone renumbers the list, so the remembered
        // index refers to the old numbering: lose one from the front and every
        // position after it slides, and the application reopens confidently on
        // the wrong picture. Clamping the number cannot fix that, because the
        // number was never the thing worth remembering.
        let showing = self.session.get(self.session_index).map(PathBuf::from);
        let paths: Vec<PathBuf> = self
            .session
            .iter()
            .map(PathBuf::from)
            .filter(|p| p.is_file())
            .collect();
        let index = showing
            .and_then(|showing| paths.iter().position(|p| *p == showing))
            // The one that was showing is itself gone. Falling back on the
            // clamped position at least lands somewhere near where you were.
            .unwrap_or_else(|| self.session_index.min(paths.len().saturating_sub(1)));
        (paths, index)
    }

    /// Record the set, if it has actually changed.
    ///
    /// Guarded because this is called from the selection path, which runs on
    /// an arrow key — writing the file on every press would be a disc write
    /// per keystroke to save something that did not change.
    pub fn remember_session(&mut self, paths: &[&Path], index: usize, support: &Support) {
        let next: Vec<String> = paths.iter().map(|p| p.display().to_string()).collect();
        if next == self.session && index == self.session_index {
            return;
        }
        self.session = next;
        self.session_index = index;
        self.save(support);
    }

    pub fn is_favourite(&self, key: &str) -> bool {
        self.favourites.iter().any(|k| k == key)
    }

    /// Star or unstar an effect, and write the change out.
    ///
    /// Saved immediately rather than on exit: the window can be closed by the
    /// operating system, by a crash, or by a user who does not think of
    /// starring as something that needs committing.
    pub fn toggle_favourite(&mut self, key: &str, support: &Support) {
        match self.favourites.iter().position(|k| k == key) {
            Some(i) => {
                self.favourites.remove(i);
            }
            None => self.favourites.push(key.to_string()),
        }
        self.save(support);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A star has to outlive the process that made it, which is a claim about
    /// a file on disc and cannot be made without one.
    #[test]
    fn a_star_survives_the_window_closing() {
        let dir = tempfile::tempdir().unwrap();
        let support = Support::at(dir.path().join("Kroma"));

        let mut s = Settings::load(&support);
        assert!(!s.is_favourite("grain"), "nothing is starred to begin with");
        s.toggle_favourite("grain", &support);
        drop(s);

        // The next launch.
        let again = Settings::load(&support);
        assert!(
            again.is_favourite("grain"),
            "the star did not survive being written and read back"
        );
    }

    /// Reopening is the set *and* the place in it. Remembering the photographs
    /// and forgetting which one was showing puts you back at the front of a
    /// folder of two hundred.
    #[test]
    fn the_set_that_was_open_is_remembered_with_which_one_was_showing() {
        let dir = tempfile::tempdir().unwrap();
        let support = Support::at(dir.path().join("Kroma"));
        let photos = dir.path().join("photos");
        std::fs::create_dir_all(&photos).unwrap();
        let names = ["a.jpg", "b.jpg", "c.jpg"];
        for n in names {
            std::fs::write(photos.join(n), b"x").unwrap();
        }
        let paths: Vec<PathBuf> = names.iter().map(|n| photos.join(n)).collect();
        let borrowed: Vec<&Path> = paths.iter().map(PathBuf::as_path).collect();

        let mut s = Settings::load(&support);
        s.remember_session(&borrowed, 1, &support);
        drop(s);

        let (reopened, index) = Settings::load(&support).session();
        assert_eq!(reopened, paths, "the set that was open came back wrong");
        assert_eq!(reopened[index], photos.join("b.jpg"));
    }

    /// A settings file that will not parse costs the stars, not the session:
    /// the defaults stand and nothing is thrown.
    #[test]
    fn a_settings_file_full_of_nonsense_is_ignored_rather_than_fatal() {
        let dir = tempfile::tempdir().unwrap();
        let support = Support::at(dir.path().join("Kroma"));
        let path = support.settings_path().unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"{ not json at all, and never was").unwrap();

        // Not `unwrap_or_default` at the call site — the whole point is that
        // `load` itself hands back the defaults rather than a `Result` that
        // somebody upstream has to decide what to do with mid-launch.
        let mut s = Settings::load(&support);
        assert!(s.favourites.is_empty());
        assert_eq!(s.session().0, Vec::<PathBuf>::new());
        assert_eq!(s.export, Export::default());

        // And the launch goes on: the next save replaces the rubbish rather
        // than refusing to touch it.
        s.toggle_favourite("grain", &support);
        assert!(Settings::load(&support).is_favourite("grain"));
    }

    /// What a newer build wrote is written back, so running an older one does
    /// not quietly discard it.
    #[test]
    fn something_a_later_version_wrote_is_kept() {
        let dir = tempfile::tempdir().unwrap();
        let support = Support::at(dir.path().join("Kroma"));
        let path = support.settings_path().unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        // Written by hand, with a key this build has never heard of.
        std::fs::write(
            &path,
            br#"{"favourites":["grain"],"lens_profiles":{"enabled":true,"kind":"auto"}}"#,
        )
        .unwrap();

        let mut s = Settings::load(&support);
        assert!(s.is_favourite("grain"), "the keys it does know were read");
        // Change something else, and write the file back out.
        s.toggle_favourite("halation", &support);

        let back = std::fs::read_to_string(&path).unwrap();
        let json: serde_json::Value = serde_json::from_str(&back).unwrap();
        assert_eq!(
            json.get("lens_profiles"),
            Some(&serde_json::json!({"enabled": true, "kind": "auto"})),
            "an older build discarded what a newer one wrote: {back}"
        );
        assert!(back.contains("halation"), "the new star was not written");
    }

    /// With nowhere to write, everything still works and nothing is lost that
    /// was not already going to be.
    #[test]
    fn settings_with_no_support_directory_are_defaults_and_do_not_panic() {
        let nowhere = Support::default();
        assert!(nowhere.settings_path().is_none());

        let mut s = Settings::load(&nowhere);
        assert!(s.favourites.is_empty());

        // Each of these writes the file out. None of them has a file.
        s.toggle_favourite("grain", &nowhere);
        s.remember_session(&[Path::new("/some/photo.jpg")], 0, &nowhere);
        s.save(&nowhere);

        // In memory it all still works; it simply does not outlive the run.
        assert!(s.is_favourite("grain"));
        assert!(!Settings::load(&nowhere).is_favourite("grain"));
    }

    /// The browser lists favourites in a group of their own and every other
    /// effect below. A key in the list twice would be two tiles for one
    /// effect, one of them wrong the moment the star on the other is clicked.
    #[test]
    fn starring_the_same_effect_twice_does_not_star_it_twice() {
        let nowhere = Support::default();
        let mut s = Settings::default();
        s.toggle_favourite("grain", &nowhere);
        // The second press is the unstar, the third stars it again.
        s.toggle_favourite("grain", &nowhere);
        s.toggle_favourite("grain", &nowhere);
        assert_eq!(s.favourites, ["grain"]);
    }

    /// Reopening has to land on the photograph you left, not on whatever has
    /// slid into its old position.
    ///
    /// Real files, because the filtering is `is_file` and a test that stubbed
    /// that out would be testing the wrong function.
    #[test]
    fn a_deleted_photograph_does_not_shift_which_one_reopens() {
        let dir = std::env::temp_dir().join("kroma-session-test");
        std::fs::create_dir_all(&dir).unwrap();
        let names = ["a.jpg", "b.jpg", "c.jpg"];
        for n in names {
            std::fs::write(dir.join(n), b"x").unwrap();
        }
        // One that is gone, sitting in front of the others.
        let missing = dir.join("deleted.jpg");
        let _ = std::fs::remove_file(&missing);

        let s = Settings {
            session: std::iter::once(missing.display().to_string())
                .chain(names.iter().map(|n| dir.join(n).display().to_string()))
                .collect(),
            // "c.jpg" — third of the survivors, fourth in the remembered list.
            session_index: 3,
            ..Default::default()
        };

        let (paths, index) = s.session();
        assert_eq!(paths.len(), 3, "the missing photograph should be dropped");
        assert_eq!(
            paths[index],
            dir.join("c.jpg"),
            "reopened on the wrong photograph after one was deleted from the front"
        );
    }

    /// And when the photograph you were on is itself the one that has gone,
    /// it still has to open something rather than give up.
    #[test]
    fn losing_the_current_photograph_still_opens_the_set() {
        let dir = std::env::temp_dir().join("kroma-session-gone");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("kept.jpg"), b"x").unwrap();
        let gone = dir.join("gone.jpg");
        let _ = std::fs::remove_file(&gone);

        let s = Settings {
            session: vec![
                dir.join("kept.jpg").display().to_string(),
                gone.display().to_string(),
            ],
            session_index: 1,
            ..Default::default()
        };

        let (paths, index) = s.session();
        assert_eq!(paths.len(), 1);
        assert!(index < paths.len(), "index {index} is off the end");
        assert_eq!(paths[index], dir.join("kept.jpg"));
    }

    #[test]
    fn starring_and_unstarring_are_the_same_gesture() {
        let mut s = Settings::default();
        assert!(!s.is_favourite("grain"));
        s.favourites.push("grain".into());
        assert!(s.is_favourite("grain"));
        let i = s.favourites.iter().position(|k| k == "grain").unwrap();
        s.favourites.remove(i);
        assert!(!s.is_favourite("grain"));
    }

    /// A settings file from a newer build must survive being loaded and saved
    /// by an older one, for the same reason a document must.
    #[test]
    fn unknown_fields_survive_a_round_trip() {
        let json = r#"{"favourites":["grain"],"future_thing":{"a":1}}"#;
        let s: Settings = serde_json::from_str(json).unwrap();
        assert!(s.is_favourite("grain"));
        let back = serde_json::to_string(&s).unwrap();
        assert!(
            back.contains("future_thing"),
            "newer data was dropped: {back}"
        );
    }

    /// A photograph that has been moved since the last run is dropped rather
    /// than reopened as an error. There is nobody to tell yet — the window
    /// does not exist.
    #[test]
    fn a_session_naming_a_missing_file_comes_back_empty() {
        let s = Settings {
            session: vec!["Z:/no/such/photo.jpg".into()],
            session_index: 0,
            ..Default::default()
        };
        let (paths, index) = s.session();
        assert!(paths.is_empty());
        assert_eq!(index, 0);
    }

    /// And the index cannot point past the end of what survived.
    #[test]
    fn the_index_is_clamped_to_what_is_left() {
        let s = Settings {
            session: vec!["Z:/gone/a.jpg".into(), "Z:/gone/b.jpg".into()],
            session_index: 1,
            ..Default::default()
        };
        let (paths, index) = s.session();
        assert!(index < paths.len().max(1));
    }

    /// Nothing here is worth interrupting a launch over.
    #[test]
    fn a_corrupt_file_gives_the_defaults_rather_than_an_error() {
        let s: Settings = serde_json::from_str("{ not json").unwrap_or_default();
        assert!(s.favourites.is_empty());
    }

    /// The rename from PhotoEditor to Kroma must not have cost anybody their
    /// stars: a settings file left in the old directory is read when the new
    /// one has nothing yet, and moves to the new one at the next save.
    #[test]
    fn a_file_left_in_the_old_directory_is_still_read() {
        let dir = tempfile::tempdir().unwrap();
        let support = Support::at(dir.path().join("Kroma"));
        let former = dir.path().join("PhotoEditor");
        std::fs::create_dir_all(&former).unwrap();
        std::fs::write(former.join("settings.json"), br#"{"favourites":["grain"]}"#).unwrap();

        let mut s = Settings::load(&support);
        assert!(s.is_favourite("grain"), "the old file was not read");

        s.toggle_favourite("halation", &support);
        let moved = std::fs::read_to_string(support.settings_path().unwrap()).unwrap();
        assert!(moved.contains("grain") && moved.contains("halation"));
    }
}
