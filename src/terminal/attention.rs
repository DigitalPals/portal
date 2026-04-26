//! Detection for terminal-emitted attention signals.
//!
//! This intentionally watches the raw byte stream before terminal rendering so
//! app-level notification escape sequences can be handled without changing the
//! terminal emulator.

use alacritty_terminal::vte::{Parser, Perform};

const MAX_NOTIFICATION_TEXT_CHARS: usize = 240;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttentionSignal {
    pub title: Option<String>,
    pub body: Option<String>,
}

impl AttentionSignal {
    fn bell() -> Self {
        Self {
            title: None,
            body: None,
        }
    }

    fn notification(title: Option<String>, body: Option<String>) -> Option<Self> {
        if title.as_deref().is_none_or(str::is_empty) && body.as_deref().is_none_or(str::is_empty) {
            return None;
        }

        Some(Self { title, body })
    }
}

pub struct AttentionParser {
    parser: Parser,
    pending: Vec<AttentionSignal>,
}

impl AttentionParser {
    pub fn new() -> Self {
        Self {
            parser: Parser::new(),
            pending: Vec::new(),
        }
    }

    pub fn advance(&mut self, bytes: &[u8]) -> Vec<AttentionSignal> {
        let mut performer = AttentionPerformer {
            signals: &mut self.pending,
        };
        self.parser.advance(&mut performer, bytes);
        std::mem::take(&mut self.pending)
    }
}

impl Default for AttentionParser {
    fn default() -> Self {
        Self::new()
    }
}

struct AttentionPerformer<'a> {
    signals: &'a mut Vec<AttentionSignal>,
}

impl Perform for AttentionPerformer<'_> {
    fn execute(&mut self, byte: u8) {
        if byte == 0x07 {
            self.signals.push(AttentionSignal::bell());
        }
    }

    fn osc_dispatch(&mut self, params: &[&[u8]], _bell_terminated: bool) {
        let Some(command) = params.first() else {
            return;
        };

        match *command {
            b"9" => {
                if params
                    .get(1)
                    .is_some_and(|param| param.iter().all(u8::is_ascii_digit))
                {
                    return;
                }

                let message = join_params(&params[1..]);
                if let Some(signal) = AttentionSignal::notification(None, normalize_text(&message))
                {
                    self.signals.push(signal);
                }
            }
            b"777" => {
                if params.get(1) != Some(&b"notify".as_slice()) {
                    return;
                }

                let title = params.get(2).and_then(|param| normalize_text(param));
                let body = if params.len() > 3 {
                    normalize_text(&join_params(&params[3..]))
                } else {
                    None
                };

                if let Some(signal) = AttentionSignal::notification(title, body) {
                    self.signals.push(signal);
                }
            }
            _ => {}
        }
    }
}

fn join_params(params: &[&[u8]]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for (idx, param) in params.iter().enumerate() {
        if idx > 0 {
            bytes.push(b';');
        }
        bytes.extend_from_slice(param);
    }
    bytes
}

fn normalize_text(bytes: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(bytes)
        .chars()
        .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
        .take(MAX_NOTIFICATION_TEXT_CHARS)
        .collect::<String>()
        .trim()
        .to_string();

    (!text.is_empty()).then_some(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_plain_bell() {
        let mut parser = AttentionParser::new();

        let signals = parser.advance(b"\x07");

        assert_eq!(signals, vec![AttentionSignal::bell()]);
    }

    #[test]
    fn detects_osc_9_notification_split_across_chunks() {
        let mut parser = AttentionParser::new();

        assert!(parser.advance(b"\x1b]9;Codex is").is_empty());
        let signals = parser.advance(b" waiting\x1b\\");

        assert_eq!(
            signals,
            vec![AttentionSignal {
                title: None,
                body: Some("Codex is waiting".to_string()),
            }]
        );
    }

    #[test]
    fn detects_osc_777_notification_split_across_chunks() {
        let mut parser = AttentionParser::new();

        assert!(parser.advance(b"\x1b]777;notify;Claude").is_empty());
        let signals = parser.advance(b" Code;Needs approval\x07");

        assert_eq!(
            signals,
            vec![AttentionSignal {
                title: Some("Claude Code".to_string()),
                body: Some("Needs approval".to_string()),
            }]
        );
    }

    #[test]
    fn ignores_osc_9_progress() {
        let mut parser = AttentionParser::new();

        let signals = parser.advance(b"\x1b]9;4;1;50\x07");

        assert!(signals.is_empty());
    }

    #[test]
    fn ignores_numeric_osc_9_subcommands() {
        let mut parser = AttentionParser::new();

        let signals = parser.advance(b"\x1b]9;9;/home/john/project\x07");

        assert!(signals.is_empty());
    }

    #[test]
    fn preserves_semicolons_in_notification_body() {
        let mut parser = AttentionParser::new();

        let signals = parser.advance(b"\x1b]777;notify;Agent;Pick A;B;or C\x1b\\");

        assert_eq!(
            signals,
            vec![AttentionSignal {
                title: Some("Agent".to_string()),
                body: Some("Pick A;B;or C".to_string()),
            }]
        );
    }
}
