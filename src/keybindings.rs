use std::fmt;

use iced::keyboard::{self, Key, Modifiers};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AppAction {
    NewConnection,
    CloseSession,
    NewTab,
    NextSession,
    PreviousSession,
    Copy,
    Paste,
    ToggleFullscreen,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct ModifierState {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub super_key: bool,
}

impl ModifierState {
    fn matches(&self, modifiers: &Modifiers) -> bool {
        self.ctrl == modifiers.control()
            && self.shift == modifiers.shift()
            && self.alt == modifiers.alt()
            && self.super_key == modifiers.logo()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeybindingKey {
    Character(char),
    Tab,
    Insert,
    F(u8),
}

impl KeybindingKey {
    fn matches(&self, key: &Key) -> bool {
        match self {
            KeybindingKey::Character(expected) => match key {
                Key::Character(c) => c
                    .chars()
                    .next()
                    .map(|ch| ch.eq_ignore_ascii_case(expected))
                    .unwrap_or(false),
                _ => false,
            },
            KeybindingKey::Tab => matches!(key, Key::Named(keyboard::key::Named::Tab)),
            KeybindingKey::Insert => matches!(key, Key::Named(keyboard::key::Named::Insert)),
            KeybindingKey::F(n) => match key {
                Key::Named(named) => matches!(
                    (n, named),
                    (1, keyboard::key::Named::F1)
                        | (2, keyboard::key::Named::F2)
                        | (3, keyboard::key::Named::F3)
                        | (4, keyboard::key::Named::F4)
                        | (5, keyboard::key::Named::F5)
                        | (6, keyboard::key::Named::F6)
                        | (7, keyboard::key::Named::F7)
                        | (8, keyboard::key::Named::F8)
                        | (9, keyboard::key::Named::F9)
                        | (10, keyboard::key::Named::F10)
                        | (11, keyboard::key::Named::F11)
                        | (12, keyboard::key::Named::F12)
                ),
                _ => false,
            },
        }
    }

    fn display(&self) -> String {
        match self {
            KeybindingKey::Character(ch) => ch.to_ascii_uppercase().to_string(),
            KeybindingKey::Tab => "Tab".to_string(),
            KeybindingKey::Insert => "Insert".to_string(),
            KeybindingKey::F(n) => format!("F{}", n),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct KeyCombo {
    key: KeybindingKey,
    modifiers: ModifierState,
}

impl KeyCombo {
    pub fn matches(&self, key: &Key, modifiers: &Modifiers) -> bool {
        self.modifiers.matches(modifiers) && self.key.matches(key)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Keybinding(KeyCombo);

impl Keybinding {
    pub fn parse(input: &str) -> Result<Self, KeybindingParseError> {
        let mut modifiers = ModifierState::default();
        let mut key: Option<KeybindingKey> = None;

        for raw in input.split('+') {
            let token = raw.trim();
            if token.is_empty() {
                continue;
            }

            let lower = token.to_ascii_lowercase();
            match lower.as_str() {
                "ctrl" | "control" => modifiers.ctrl = true,
                "shift" => modifiers.shift = true,
                "alt" => modifiers.alt = true,
                "super" | "logo" | "cmd" | "command" | "meta" => modifiers.super_key = true,
                _ => {
                    if key.is_some() {
                        return Err(KeybindingParseError::MultipleKeys);
                    }
                    key = Some(parse_key_token(token)?);
                }
            }
        }

        let key = key.ok_or(KeybindingParseError::MissingKey)?;
        Ok(Keybinding(KeyCombo { key, modifiers }))
    }

    pub fn matches(&self, key: &Key, modifiers: &Modifiers) -> bool {
        self.0.matches(key, modifiers)
    }
}

impl fmt::Display for Keybinding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut parts = Vec::new();
        if self.0.modifiers.ctrl {
            parts.push("Ctrl".to_string());
        }
        if self.0.modifiers.shift {
            parts.push("Shift".to_string());
        }
        if self.0.modifiers.alt {
            parts.push("Alt".to_string());
        }
        if self.0.modifiers.super_key {
            parts.push("Super".to_string());
        }
        parts.push(self.0.key.display());
        write!(f, "{}", parts.join("+"))
    }
}

impl Serialize for Keybinding {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for Keybinding {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        Keybinding::parse(&raw).map_err(de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeybindingParseError {
    MissingKey,
    MultipleKeys,
    UnknownKey(String),
}

impl fmt::Display for KeybindingParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            KeybindingParseError::MissingKey => write!(f, "missing key"),
            KeybindingParseError::MultipleKeys => write!(f, "multiple keys specified"),
            KeybindingParseError::UnknownKey(key) => write!(f, "unknown key: {}", key),
        }
    }
}

impl std::error::Error for KeybindingParseError {}

fn parse_key_token(token: &str) -> Result<KeybindingKey, KeybindingParseError> {
    let lower = token.to_ascii_lowercase();
    if let Some(rest) = lower.strip_prefix('f') {
        if let Ok(num) = rest.parse::<u8>() {
            if (1..=12).contains(&num) {
                return Ok(KeybindingKey::F(num));
            }
        }
    }

    match lower.as_str() {
        "tab" => Ok(KeybindingKey::Tab),
        "insert" => Ok(KeybindingKey::Insert),
        _ => {
            let mut chars = token.chars();
            if let (Some(first), None) = (chars.next(), chars.next()) {
                Ok(KeybindingKey::Character(first.to_ascii_lowercase()))
            } else {
                Err(KeybindingParseError::UnknownKey(token.to_string()))
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeybindingsConfig {
    #[serde(default = "default_new_connection")]
    pub new_connection: Vec<Keybinding>,
    #[serde(default = "default_close_session")]
    pub close_session: Vec<Keybinding>,
    #[serde(default = "default_new_tab")]
    pub new_tab: Vec<Keybinding>,
    #[serde(default = "default_next_session")]
    pub next_session: Vec<Keybinding>,
    #[serde(default = "default_previous_session")]
    pub previous_session: Vec<Keybinding>,
    #[serde(default = "default_terminal_copy")]
    pub terminal_copy: Vec<Keybinding>,
    #[serde(default = "default_terminal_paste")]
    pub terminal_paste: Vec<Keybinding>,
    #[serde(default = "default_toggle_fullscreen")]
    pub toggle_fullscreen: Vec<Keybinding>,
}

impl Default for KeybindingsConfig {
    fn default() -> Self {
        Self {
            new_connection: default_new_connection(),
            close_session: default_close_session(),
            new_tab: default_new_tab(),
            next_session: default_next_session(),
            previous_session: default_previous_session(),
            terminal_copy: default_terminal_copy(),
            terminal_paste: default_terminal_paste(),
            toggle_fullscreen: default_toggle_fullscreen(),
        }
    }
}

impl KeybindingsConfig {
    pub fn matches_action(&self, action: AppAction, key: &Key, modifiers: &Modifiers) -> bool {
        let bindings = match action {
            AppAction::NewConnection => &self.new_connection,
            AppAction::CloseSession => &self.close_session,
            AppAction::NewTab => &self.new_tab,
            AppAction::NextSession => &self.next_session,
            AppAction::PreviousSession => &self.previous_session,
            AppAction::Copy => &self.terminal_copy,
            AppAction::Paste => &self.terminal_paste,
            AppAction::ToggleFullscreen => &self.toggle_fullscreen,
        };

        bindings
            .iter()
            .any(|binding| binding.matches(key, modifiers))
    }
}

fn default_new_connection() -> Vec<Keybinding> {
    vec![Keybinding::parse("Ctrl+Shift+N").expect("valid default")]
}

fn default_close_session() -> Vec<Keybinding> {
    vec![Keybinding::parse("Ctrl+Shift+W").expect("valid default")]
}

fn default_new_tab() -> Vec<Keybinding> {
    vec![Keybinding::parse("Ctrl+Shift+T").expect("valid default")]
}

fn default_next_session() -> Vec<Keybinding> {
    vec![Keybinding::parse("Ctrl+Tab").expect("valid default")]
}

fn default_previous_session() -> Vec<Keybinding> {
    vec![Keybinding::parse("Ctrl+Shift+Tab").expect("valid default")]
}

fn default_terminal_copy() -> Vec<Keybinding> {
    vec![Keybinding::parse("Ctrl+Shift+C").expect("valid default")]
}

fn default_terminal_paste() -> Vec<Keybinding> {
    vec![Keybinding::parse("Ctrl+Shift+V").expect("valid default")]
}

fn default_toggle_fullscreen() -> Vec<Keybinding> {
    vec![
        Keybinding::parse("Ctrl+Shift+F").expect("valid default"),
        Keybinding::parse("F11").expect("valid default"),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_ctrl_shift_letter() {
        let binding = Keybinding::parse("Ctrl+Shift+N").unwrap();
        assert_eq!(binding.to_string(), "Ctrl+Shift+N");
    }

    #[test]
    fn parse_function_key() {
        let binding = Keybinding::parse("F11").unwrap();
        assert_eq!(binding.to_string(), "F11");
    }

    #[test]
    fn matches_case_insensitive_character() {
        let binding = Keybinding::parse("Ctrl+Shift+N").unwrap();
        let key = Key::Character("n".into());
        let modifiers = Modifiers::CTRL | Modifiers::SHIFT;
        assert!(binding.matches(&key, &modifiers));

        let key_upper = Key::Character("N".into());
        assert!(binding.matches(&key_upper, &modifiers));
    }

    #[test]
    fn matches_function_key() {
        let binding = Keybinding::parse("F11").unwrap();
        let key = Key::Named(keyboard::key::Named::F11);
        let modifiers = Modifiers::default();
        assert!(binding.matches(&key, &modifiers));
    }

    #[test]
    fn modifiers_must_match_exactly() {
        let binding = Keybinding::parse("Ctrl+Tab").unwrap();
        let key = Key::Named(keyboard::key::Named::Tab);
        let modifiers = Modifiers::CTRL | Modifiers::SHIFT;
        assert!(!binding.matches(&key, &modifiers));
    }
}
