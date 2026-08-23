//! Where the application keeps what belongs to it rather than to a photograph.
//!
//! Given by the host, never guessed. Rust cannot know that a Mac wants
//! `~/Library/Application Support`, that Windows wants `%APPDATA%`, and that an
//! iPad wants a container path which does not exist until the process starts.
//! A `cfg!` that tries is a `cfg!` sitting in code whose whole purpose is to be
//! platform-independent — and it was already wrong, silently, on the Mac.
//!
//! Unset means *write nothing*. A host that has not said where has not agreed
//! to anything being written, and a default that guesses would be an
//! application putting files somewhere nobody chose.

use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Support {
    root: Option<PathBuf>,
}

impl Support {
    /// Keep our files under `root`. The host supplies this once, at start-up.
    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self {
            root: Some(root.into()),
        }
    }

    pub fn root(&self) -> Option<&Path> {
        self.root.as_deref()
    }

    /// Where autosaved work in progress is kept, one file per photograph.
    pub fn edits_dir(&self) -> Option<PathBuf> {
        Some(self.root.as_ref()?.join("edits"))
    }

    /// Where the things belonging to the person rather than to a picture live.
    pub fn settings_path(&self) -> Option<PathBuf> {
        Some(self.root.as_ref()?.join("settings.json"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_support_directory_nobody_set_yields_no_paths() {
        // Nothing is written until the host says where. A default that guesses
        // is how an application ends up sprinkling files somewhere nobody
        // asked for, on a platform nobody tested.
        let s = Support::default();
        assert!(s.root().is_none());
        assert!(s.edits_dir().is_none());
        assert!(s.settings_path().is_none());
    }

    #[test]
    fn the_paths_hang_off_the_root_the_host_gave() {
        let s = Support::at("/Users/someone/Library/Application Support/Kroma");
        assert_eq!(
            s.edits_dir().unwrap(),
            std::path::Path::new("/Users/someone/Library/Application Support/Kroma/edits")
        );
        assert_eq!(
            s.settings_path().unwrap(),
            std::path::Path::new("/Users/someone/Library/Application Support/Kroma/settings.json")
        );
    }
}
