//! What the application remembers between runs.
//!
//! Not the edit — that is the document, and it lives beside the photograph.
//! This is the handful of things that belong to the *person* rather than to
//! any one picture: which effects they have starred, and whatever joins that
//! list later.
//!
//! A favourite that vanishes when the window closes is half a feature, which
//! is the whole reason this file exists. It is deliberately small and
//! deliberately forgiving — a settings file that fails to parse costs the user
//! their stars, not their session, so every error here is swallowed and the
//! defaults stand.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

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
    /// Anything a newer build wrote that this one does not know about, kept
    /// so that running an older version does not silently discard it.
    #[serde(flatten)]
    unknown: serde_json::Map<String, serde_json::Value>,
}

impl Settings {
    /// Where the file lives, per platform.
    ///
    /// Windows now, macOS later — the second arm is here because getting it
    /// wrong is invisible until the port, and it is three lines.
    fn path() -> Option<PathBuf> {
        let dir = if cfg!(windows) {
            PathBuf::from(std::env::var_os("APPDATA")?)
        } else {
            match std::env::var_os("XDG_CONFIG_HOME") {
                Some(v) => PathBuf::from(v),
                None => PathBuf::from(std::env::var_os("HOME")?).join(".config"),
            }
        };
        Some(dir.join("Kroma").join("settings.json"))
    }

    pub fn load() -> Self {
        // Every failure here is the same failure: there are no settings yet.
        // A missing file, an unreadable one and a corrupt one all mean the
        // user gets the defaults, and none of them is worth interrupting a
        // launch over.
        // The old location is read when the new one is not there yet, so
        // the rename to Kroma does not quietly cost anyone their stars. It
        // moves to the new path the next time anything is saved.
        let former = Self::path()
            .and_then(|p| p.parent()?.parent().map(|d| d.join("PhotoEditor")))
            .map(|d| d.join("settings.json"));
        Self::path()
            .into_iter()
            .chain(former)
            .filter_map(|p| std::fs::read_to_string(p).ok())
            .find_map(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let Some(path) = Self::path() else {
            return;
        };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(path, json);
        }
    }

    /// What to reopen, and where in it to start.
    ///
    /// Filtered by what still exists. A photograph moved or deleted since the
    /// last run must not stop the application starting, and silently leaving
    /// it out is the only reasonable thing to do about it — there is nobody
    /// to tell yet.
    pub fn session(&self) -> (Vec<PathBuf>, usize) {
        let paths: Vec<PathBuf> = self
            .session
            .iter()
            .map(PathBuf::from)
            .filter(|p| p.is_file())
            .collect();
        let index = self.session_index.min(paths.len().saturating_sub(1));
        (paths, index)
    }

    /// Record the set, if it has actually changed.
    ///
    /// Guarded because this is called from the selection path, which runs on
    /// an arrow key — writing the file on every press would be a disc write
    /// per keystroke to save something that did not change.
    pub fn remember_session(&mut self, paths: &[&Path], index: usize) {
        let next: Vec<String> = paths.iter().map(|p| p.display().to_string()).collect();
        if next == self.session && index == self.session_index {
            return;
        }
        self.session = next;
        self.session_index = index;
        self.save();
    }

    pub fn is_favourite(&self, key: &str) -> bool {
        self.favourites.iter().any(|k| k == key)
    }

    /// Star or unstar an effect, and write the change out.
    ///
    /// Saved immediately rather than on exit: the window can be closed by the
    /// operating system, by a crash, or by a user who does not think of
    /// starring as something that needs committing.
    pub fn toggle_favourite(&mut self, key: &str) {
        match self.favourites.iter().position(|k| k == key) {
            Some(i) => {
                self.favourites.remove(i);
            }
            None => self.favourites.push(key.to_string()),
        }
        self.save();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
