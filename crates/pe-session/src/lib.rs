//! The workflow layer.
//!
//! Between the engine and the shells. It owns the things that are neither
//! image processing nor interface: which photograph is open, what its edit is,
//! where work in progress is kept, and the rules about what may be written
//! where.
//!
//! Those rules lived in `apps/windows` until the Mac port needed them too, and
//! a rule implemented twice is a rule that will differ. "Never write over an
//! original" is not a Windows rule.

pub mod autosave;
pub mod describe;
pub mod export;
pub mod library;
pub mod scopes;
pub mod session;
pub mod settings;
pub mod support;
pub mod surface;

pub use library::{Library, Thumbnail};
pub use scopes::Scopes;
pub use session::{Compare, Session, SessionError};
pub use settings::Settings;
pub use support::Support;
pub use surface::Attached;
