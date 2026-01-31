//! VNC client module for Portal
//!
//! Provides VNC connection, framebuffer management, and input handling.

pub mod keysym;
mod encoding;
pub mod framebuffer;
mod net;
pub mod session;
pub mod widget;

pub use session::VncSession;
