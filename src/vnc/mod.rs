//! VNC client module for Portal
//!
//! Provides VNC connection, framebuffer management, and input handling.

mod encoding;
pub mod framebuffer;
pub mod keysym;
mod net;
pub mod session;
pub mod widget;

pub use session::VncSession;
