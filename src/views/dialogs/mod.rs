pub mod common;
pub mod host_dialog;
pub mod host_key_dialog;
pub mod settings_dialog;
pub mod snippets_dialog;

// Re-export common utilities for convenience
pub use common::{dialog_backdrop, primary_button_style, secondary_button_style};
