//! Terminal link detection (URLs and file paths)
//!
//! Finds the URL or file path under a grid point so the terminal widget can
//! underline it on Ctrl+hover and open it on Ctrl+click. Detection combines
//! explicit OSC 8 hyperlinks on cells with a regex scan of the visible
//! viewport (agent CLIs like Claude Code and Codex print plain
//! `src/foo.rs:123`-style references and URLs).

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Boundary, Column, Direction, Line, Point};
use alacritty_terminal::term::Term;
use alacritty_terminal::term::search::{Match, RegexIter, RegexSearch};

/// Extra lines beyond the viewport to search so links on wrapped lines that
/// cross the viewport edge are still found in full.
const MAX_LINK_WRAP_LINES: i32 = 8;

/// A link found in terminal content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalLink {
    /// A URL to open externally (scheme included).
    Url(String),
    /// A file path to open in the built-in viewer, with an optional
    /// 1-based line number parsed from a `path:line[:column]` suffix.
    FilePath { path: String, line: Option<u32> },
}

/// A detected link and the grid range it covers (for underlining).
#[derive(Debug, Clone)]
pub struct LinkMatch {
    pub link: TerminalLink,
    pub range: Match,
}

/// A visible span in screen cell coordinates: (line, start_col, end_col).
pub type ScreenSpan = (usize, usize, usize);

/// Combined URL + file path pattern, URL alternative first so a path-like
/// tail inside a URL never matches on its own. File paths come in three
/// shapes: prefixed (`/abs`, `~/home`, `./rel`, `../up`), bare relative with
/// at least one slash, and bare file names with an extension. Each may carry
/// a `:line[:column]` suffix.
const LINK_PATTERN: &str = concat!(
    r"(?:https?|file)://[^\s<>\x22'\x60\x00-\x1f\x7f]+",
    r"|(?:/|~/|\.\.?/)[A-Za-z0-9_.@+-]+(?:/[A-Za-z0-9_.@+-]+)*(?::\d+(?::\d+)?)?",
    r"|[A-Za-z0-9_.@+-]+(?:/[A-Za-z0-9_.@+-]+)+(?::\d+(?::\d+)?)?",
    r"|\.?[A-Za-z0-9_@+-][A-Za-z0-9_.@+-]*\.[A-Za-z][A-Za-z0-9]{0,9}(?::\d+(?::\d+)?)?",
);

/// Regex used for link scans, wrapped so widget state stays `Debug`.
pub struct LinkRegex(RegexSearch);

impl LinkRegex {
    pub fn new() -> Option<Self> {
        match RegexSearch::new(LINK_PATTERN) {
            Ok(regex) => Some(Self(regex)),
            Err(error) => {
                tracing::error!("Terminal link pattern rejected: {error}");
                None
            }
        }
    }
}

impl std::fmt::Debug for LinkRegex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LinkRegex").finish_non_exhaustive()
    }
}

impl Default for LinkRegex {
    fn default() -> Self {
        Self::new().expect("link pattern must compile")
    }
}

/// Find the link under `point` (buffer coordinates), if any.
///
/// OSC 8 hyperlinks on the cell win over regex detection since they carry an
/// explicit target.
pub fn link_at_point<T>(term: &Term<T>, regex: &mut LinkRegex, point: Point) -> Option<LinkMatch> {
    hyperlink_at_point(term, point).or_else(|| regex_link_at_point(term, &mut regex.0, point))
}

/// Convert a match range to visible screen spans `(line, start_col, end_col)`
/// for rendering, clamped to the viewport.
pub fn match_screen_spans(
    range: &Match,
    display_offset: usize,
    rows: usize,
    cols: usize,
) -> Vec<ScreenSpan> {
    let mut spans = Vec::new();
    if cols == 0 || rows == 0 {
        return spans;
    }

    let offset = display_offset as i32;
    let start = *range.start();
    let end = *range.end();
    let first_line = start.line.0.max(-offset);
    let last_line = end.line.0.min(rows as i32 - 1 - offset);

    for line in first_line..=last_line {
        let screen_line = (line + offset) as usize;
        let start_col = if line == start.line.0 {
            start.column.0.min(cols - 1)
        } else {
            0
        };
        let end_col = if line == end.line.0 {
            end.column.0.min(cols - 1)
        } else {
            cols - 1
        };
        spans.push((screen_line, start_col, end_col));
    }

    spans
}

/// Detect an explicit OSC 8 hyperlink on the cell under `point`, expanding
/// the underline span along contiguous cells of the same hyperlink.
fn hyperlink_at_point<T>(term: &Term<T>, point: Point) -> Option<LinkMatch> {
    let grid = term.grid();
    let hyperlink = grid[point].hyperlink()?;

    let mut start = point;
    let mut end = point;
    while start.column.0 > 0 {
        let previous = Point::new(start.line, Column(start.column.0 - 1));
        if grid[previous].hyperlink().as_ref() == Some(&hyperlink) {
            start = previous;
        } else {
            break;
        }
    }
    let last_column = term.last_column();
    while end.column < last_column {
        let next = Point::new(end.line, Column(end.column.0 + 1));
        if grid[next].hyperlink().as_ref() == Some(&hyperlink) {
            end = next;
        } else {
            break;
        }
    }

    Some(LinkMatch {
        link: classify_uri(hyperlink.uri()),
        range: start..=end,
    })
}

/// Scan the visible viewport (expanded to wrapped-line boundaries) and return
/// the regex match containing `point`.
fn regex_link_at_point<T>(
    term: &Term<T>,
    regex: &mut RegexSearch,
    point: Point,
) -> Option<LinkMatch> {
    let display_offset = term.grid().display_offset() as i32;
    let viewport_top = Line(-display_offset);
    let viewport_bottom = viewport_top + term.screen_lines().saturating_sub(1);

    // Expand to wrapped-line boundaries so matches crossing the viewport edge
    // are found in full, bounded to keep pathological wrap chains cheap.
    let mut start = term.line_search_left(Point::new(viewport_top, Column(0)));
    let mut end = term.line_search_right(Point::new(viewport_bottom, Column(0)));
    start.line = start.line.max(Line(viewport_top.0 - MAX_LINK_WRAP_LINES));
    end.line = end.line.min(Line(viewport_bottom.0 + MAX_LINK_WRAP_LINES));

    for regex_match in RegexIter::new(start, end, Direction::Right, term, regex) {
        if *regex_match.start() > point {
            break;
        }
        if !regex_match.contains(&point) {
            continue;
        }

        let text = term.bounds_to_string(*regex_match.start(), *regex_match.end());
        let trimmed_len = trimmed_link_len(&text);
        if trimmed_len == 0 {
            return None;
        }

        // The matched text is ASCII (enforced by the pattern), so characters
        // map 1:1 to grid cells and the range can be shrunk by char count.
        let removed = text.chars().count() - trimmed_len;
        let end_point = if removed > 0 {
            regex_match.end().sub(term, Boundary::Grid, removed)
        } else {
            *regex_match.end()
        };
        let range = *regex_match.start()..=end_point;
        if !range.contains(&point) {
            return None;
        }

        let trimmed: String = text.chars().take(trimmed_len).collect();
        return Some(LinkMatch {
            link: classify_text(&trimmed),
            range,
        });
    }

    None
}

/// Length in chars after stripping trailing punctuation that is almost
/// always sentence context rather than part of the link. A closing paren is
/// kept when the link contains an opening one (Wikipedia-style URLs).
fn trimmed_link_len(text: &str) -> usize {
    let mut len = text.chars().count();
    let chars: Vec<char> = text.chars().collect();
    while len > 0 {
        match chars[len - 1] {
            '.' | ',' | ';' | ':' | '!' | '?' => len -= 1,
            ')' => {
                let kept = &chars[..len];
                let opens = kept.iter().filter(|&&c| c == '(').count();
                let closes = kept.iter().filter(|&&c| c == ')').count();
                if closes > opens {
                    len -= 1;
                } else {
                    break;
                }
            }
            _ => break,
        }
    }
    len
}

/// Classify plain matched text as a URL or file path.
fn classify_text(text: &str) -> TerminalLink {
    let lower = text.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        return TerminalLink::Url(text.to_string());
    }
    if lower.starts_with("file://") {
        return classify_uri(text);
    }

    let (path, line) = split_line_suffix(text);
    TerminalLink::FilePath {
        path: path.to_string(),
        line,
    }
}

/// Classify an explicit URI (from OSC 8 or a `file://` match). `file://`
/// URIs become file paths; the host part is ignored since the path is opened
/// on the session's host anyway (matching `ls --hyperlink` output on the
/// remote side).
fn classify_uri(uri: &str) -> TerminalLink {
    let lower = uri.to_ascii_lowercase();
    if let Some(rest) = lower
        .starts_with("file://")
        .then(|| &uri["file://".len()..])
    {
        let path_start = rest.find('/').unwrap_or(rest.len());
        let path = percent_decode(&rest[path_start..]);
        if !path.is_empty() {
            return TerminalLink::FilePath { path, line: None };
        }
    }
    TerminalLink::Url(uri.to_string())
}

/// Split a `path:line[:column]` suffix off a matched file path. The column,
/// if present, is parsed and discarded (the viewer only scrolls to lines).
fn split_line_suffix(text: &str) -> (&str, Option<u32>) {
    let mut path = text;
    let mut line = None;

    // Up to two numeric suffixes: strip the column first, then the line.
    for _ in 0..2 {
        let Some((head, tail)) = path.rsplit_once(':') else {
            break;
        };
        if head.is_empty() || tail.is_empty() || !tail.bytes().all(|b| b.is_ascii_digit()) {
            break;
        }
        line = tail.parse::<u32>().ok();
        path = head;
    }

    (path, line)
}

/// Decode percent-encoded bytes in a URI path (e.g. `%20` -> space).
/// Invalid escapes are kept literally.
pub fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(high), Some(low)) = (
                (bytes[i + 1] as char).to_digit(16),
                (bytes[i + 2] as char).to_digit(16),
            )
        {
            decoded.push((high * 16 + low) as u8);
            i += 3;
        } else {
            decoded.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terminal::backend::{TerminalBackend, TerminalSize};

    fn link_at(content: &[u8], line: i32, column: usize) -> Option<TerminalLink> {
        let (backend, _events) = TerminalBackend::new(TerminalSize::new(80, 10));
        backend.process_input(content);
        let term = backend.term();
        let term = term.lock();
        let mut regex = LinkRegex::new().expect("pattern compiles");
        link_at_point(&term, &mut regex, Point::new(Line(line), Column(column)))
            .map(|found| found.link)
    }

    #[test]
    fn detects_https_url_and_strips_trailing_punctuation() {
        assert_eq!(
            link_at(b"see https://github.com/foo/bar. next", 0, 10),
            Some(TerminalLink::Url("https://github.com/foo/bar".to_string()))
        );
    }

    #[test]
    fn keeps_balanced_closing_paren_in_url() {
        assert_eq!(
            link_at(b"https://en.wikipedia.org/wiki/Rust_(language)", 0, 5),
            Some(TerminalLink::Url(
                "https://en.wikipedia.org/wiki/Rust_(language)".to_string()
            ))
        );
        assert_eq!(
            link_at(b"(https://example.com/a)", 0, 5),
            Some(TerminalLink::Url("https://example.com/a".to_string()))
        );
    }

    #[test]
    fn detects_relative_path_with_line_and_column() {
        assert_eq!(
            link_at(b"error at src/app/update/session.rs:596:12 here", 0, 20),
            Some(TerminalLink::FilePath {
                path: "src/app/update/session.rs".to_string(),
                line: Some(596),
            })
        );
    }

    #[test]
    fn detects_absolute_home_and_dotted_paths() {
        assert_eq!(
            link_at(b"/etc/hosts", 0, 3),
            Some(TerminalLink::FilePath {
                path: "/etc/hosts".to_string(),
                line: None,
            })
        );
        assert_eq!(
            link_at(b"~/notes/todo.md", 0, 4),
            Some(TerminalLink::FilePath {
                path: "~/notes/todo.md".to_string(),
                line: None,
            })
        );
        assert_eq!(
            link_at(b"./run.sh", 0, 4),
            Some(TerminalLink::FilePath {
                path: "./run.sh".to_string(),
                line: None,
            })
        );
    }

    #[test]
    fn detects_bare_file_name_with_extension() {
        assert_eq!(
            link_at(b"edit Cargo.toml:12 now", 0, 8),
            Some(TerminalLink::FilePath {
                path: "Cargo.toml".to_string(),
                line: Some(12),
            })
        );
        assert_eq!(
            link_at(b"check .gitignore.bak too", 0, 9),
            Some(TerminalLink::FilePath {
                path: ".gitignore.bak".to_string(),
                line: None,
            })
        );
    }

    #[test]
    fn ignores_plain_words_and_off_link_cells() {
        assert_eq!(link_at(b"plain words only", 0, 3), None);
        // Cell after the link text.
        assert_eq!(link_at(b"src/main.rs done", 0, 13), None);
    }

    #[test]
    fn url_inside_text_is_not_split_into_a_path() {
        // Clicking the host/path tail of a URL yields the whole URL, not a
        // file path.
        assert_eq!(
            link_at(b"https://github.com/DigitalPals/portal/releases", 0, 30),
            Some(TerminalLink::Url(
                "https://github.com/DigitalPals/portal/releases".to_string()
            ))
        );
    }

    #[test]
    fn detects_link_wrapped_across_lines() {
        // 80 columns: the URL starts at column 70 and wraps to line 1.
        let mut content = Vec::new();
        content.extend_from_slice(&[b' '; 70]);
        content.extend_from_slice(b"https://example.com/some/long/path");
        assert_eq!(
            link_at(&content, 1, 10),
            Some(TerminalLink::Url(
                "https://example.com/some/long/path".to_string()
            ))
        );
    }

    #[test]
    fn file_uri_becomes_file_path_with_percent_decoding() {
        assert_eq!(
            link_at(b"file:///var/log/my%20app.log", 0, 5),
            Some(TerminalLink::FilePath {
                path: "/var/log/my app.log".to_string(),
                line: None,
            })
        );
    }

    #[test]
    fn split_line_suffix_handles_line_and_column() {
        assert_eq!(split_line_suffix("a.rs"), ("a.rs", None));
        assert_eq!(split_line_suffix("a.rs:12"), ("a.rs", Some(12)));
        assert_eq!(split_line_suffix("a.rs:12:5"), ("a.rs", Some(12)));
    }

    #[test]
    fn percent_decode_keeps_invalid_escapes() {
        assert_eq!(percent_decode("a%20b"), "a b");
        assert_eq!(percent_decode("a%2"), "a%2");
        assert_eq!(percent_decode("100%"), "100%");
    }

    #[test]
    fn match_screen_spans_clamps_to_viewport() {
        let range = Point::new(Line(-1), Column(70))..=Point::new(Line(1), Column(9));
        assert_eq!(
            match_screen_spans(&range, 0, 10, 80),
            vec![(0, 0, 79), (1, 0, 9)]
        );
        assert_eq!(
            match_screen_spans(&range, 1, 10, 80),
            vec![(0, 70, 79), (1, 0, 79), (2, 0, 9)]
        );
    }
}
