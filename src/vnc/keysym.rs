//! Iced keyboard key to X11 keysym conversion for VNC

use iced::keyboard::{Key, key::Named};

/// Convert an Iced keyboard key to an X11 keysym.
/// Returns None for keys that can't be mapped.
pub fn key_to_keysym(key: &Key) -> Option<u32> {
    match key {
        Key::Character(c) => {
            // For single characters, use the Unicode codepoint.
            // X11 keysyms for Latin-1 map directly to Unicode for 0x20..0xFF.
            // For Unicode above 0xFF, use 0x01000000 + codepoint.
            let ch = c.chars().next()?;
            let cp = ch as u32;
            if (0x20..=0xFF).contains(&cp) {
                Some(cp)
            } else if cp > 0xFF {
                Some(0x01000000 + cp)
            } else {
                None
            }
        }
        Key::Named(named) => named_key_to_keysym(named),
        Key::Unidentified => None,
    }
}

fn named_key_to_keysym(key: &Named) -> Option<u32> {
    // X11 keysym constants
    Some(match key {
        Named::Enter => 0xFF0D,
        Named::Tab => 0xFF09,
        Named::Space => 0x0020,
        Named::Backspace => 0xFF08,
        Named::Escape => 0xFF1B,
        Named::Delete => 0xFFFF,
        Named::Insert => 0xFF63,
        Named::Home => 0xFF50,
        Named::End => 0xFF57,
        Named::PageUp => 0xFF55,
        Named::PageDown => 0xFF56,
        Named::ArrowLeft => 0xFF51,
        Named::ArrowUp => 0xFF52,
        Named::ArrowRight => 0xFF53,
        Named::ArrowDown => 0xFF54,
        Named::Shift => 0xFFE1,
        Named::Control => 0xFFE3,
        Named::Alt => 0xFFE9,
        Named::Super => 0xFFEB,
        Named::CapsLock => 0xFFE5,
        Named::F1 => 0xFFBE,
        Named::F2 => 0xFFBF,
        Named::F3 => 0xFFC0,
        Named::F4 => 0xFFC1,
        Named::F5 => 0xFFC2,
        Named::F6 => 0xFFC3,
        Named::F7 => 0xFFC4,
        Named::F8 => 0xFFC5,
        Named::F9 => 0xFFC6,
        Named::F10 => 0xFFC7,
        Named::F11 => 0xFFC8,
        Named::F12 => 0xFFC9,
        _ => return None,
    })
}
