//! Local terminal session support
//!
//! Provides PTY-based local terminal sessions that run the user's shell.

mod session;

pub use session::{LocalEvent, LocalSession};
