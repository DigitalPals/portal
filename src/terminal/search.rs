//! Terminal scrollback search (find-in-buffer)
//!
//! Wraps alacritty_terminal's regex search to provide literal (escaped)
//! whole-buffer search with per-session state for the search bar UI.

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line, Point};
use alacritty_terminal::term::Term;
use alacritty_terminal::term::search::RegexSearch;

pub use alacritty_terminal::term::search::Match;

/// Cap on the number of matches collected per search to bound work on huge
/// scrollbacks. The match counter shows at most this many matches.
pub const MAX_SEARCH_MATCHES: usize = 1000;

/// Per-session terminal search state (search bar + computed matches).
#[derive(Debug, Default)]
pub struct TerminalSearchState {
    /// Whether the search bar is visible for this session.
    pub open: bool,
    /// Current query text (searched literally, not as a regex).
    pub query: String,
    /// Case-sensitive matching. Off by default.
    pub case_sensitive: bool,
    /// All matches in buffer order (topmost first), capped at
    /// [`MAX_SEARCH_MATCHES`]. Points are in grid coordinates (negative lines
    /// are scrollback).
    pub matches: Vec<Match>,
    /// Index of the active match within `matches`.
    pub current: Option<usize>,
    /// Bumped on every search-state change; the terminal widget uses it to
    /// invalidate its render cache.
    pub version: u64,
    /// Terminal render epoch the matches were computed against. New output or
    /// a resize bumps the epoch, which invalidates the match positions.
    pub last_epoch: u64,
}

impl TerminalSearchState {
    /// Mark the search state as changed so highlights are recomputed.
    pub fn bump_version(&mut self) {
        self.version = self.version.wrapping_add(1);
    }

    /// Advance to the next match, wrapping at the end of the buffer.
    pub fn select_next(&mut self) -> Option<usize> {
        if self.matches.is_empty() {
            self.current = None;
            return None;
        }
        let next = match self.current {
            Some(index) => (index + 1) % self.matches.len(),
            None => 0,
        };
        self.current = Some(next);
        Some(next)
    }

    /// Go back to the previous match, wrapping at the start of the buffer.
    pub fn select_previous(&mut self) -> Option<usize> {
        if self.matches.is_empty() {
            self.current = None;
            return None;
        }
        let len = self.matches.len();
        let previous = match self.current {
            Some(index) => (index + len - 1) % len,
            None => len - 1,
        };
        self.current = Some(previous);
        Some(previous)
    }

    /// Counter label for the search bar ("3/17", "No matches"), or `None`
    /// when the query is empty.
    pub fn counter_label(&self) -> Option<String> {
        if self.query.is_empty() {
            return None;
        }
        if self.matches.is_empty() {
            return Some("No matches".to_string());
        }
        let current = self.current.map(|index| index + 1).unwrap_or(0);
        Some(format!("{}/{}", current, self.matches.len()))
    }

    /// The active match, if any.
    pub fn current_match(&self) -> Option<&Match> {
        self.current.and_then(|index| self.matches.get(index))
    }
}

/// Build the regex pattern for a literal query with explicit case flags.
pub fn search_pattern(query: &str, case_sensitive: bool) -> String {
    // alacritty's RegexSearch applies smart-case on its own; set the flag
    // explicitly so the toggle always wins.
    let flag = if case_sensitive { "(?-i)" } else { "(?i)" };
    format!("{flag}{}", regex::escape(query))
}

/// Find all literal matches of `query` in the whole buffer (scrollback +
/// viewport), in buffer order, capped at `max_matches`.
pub fn find_matches<T>(
    term: &Term<T>,
    query: &str,
    case_sensitive: bool,
    max_matches: usize,
) -> Vec<Match> {
    if query.is_empty() || max_matches == 0 {
        return Vec::new();
    }

    let pattern = search_pattern(query, case_sensitive);
    let mut regex = match RegexSearch::new(&pattern) {
        Ok(regex) => regex,
        Err(error) => {
            tracing::debug!("Terminal search pattern rejected: {error}");
            return Vec::new();
        }
    };

    let last_column = term.last_column();
    let bottommost_line = term.bottommost_line();
    let end = Point::new(bottommost_line, last_column);
    let mut origin = Point::new(term.topmost_line(), Column(0));
    let mut matches: Vec<Match> = Vec::new();

    while matches.len() < max_matches {
        let Some(regex_match) = term.regex_search_right(&mut regex, origin, end) else {
            break;
        };

        // Guard against a non-advancing search (defensive; cannot happen for
        // non-empty literal queries).
        if matches
            .last()
            .is_some_and(|last| regex_match.start() <= last.start())
        {
            break;
        }

        let match_end = *regex_match.end();
        matches.push(regex_match);

        origin = if match_end.column < last_column {
            Point::new(match_end.line, Column(match_end.column.0 + 1))
        } else if match_end.line < bottommost_line {
            Point::new(Line(match_end.line.0 + 1), Column(0))
        } else {
            break;
        };
    }

    matches
}

/// Pick the initial active match after a query change: the last match at or
/// above the bottom of the current viewport (nearest going backwards, like a
/// terminal search starting from the prompt), falling back to the first match.
pub fn initial_match_index(matches: &[Match], viewport_bottom_line: i32) -> Option<usize> {
    if matches.is_empty() {
        return None;
    }
    Some(
        matches
            .iter()
            .rposition(|regex_match| regex_match.start().line.0 <= viewport_bottom_line)
            .unwrap_or(0),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terminal::backend::{TerminalBackend, TerminalSize};

    fn match_coords(regex_match: &Match) -> ((i32, usize), (i32, usize)) {
        (
            (regex_match.start().line.0, regex_match.start().column.0),
            (regex_match.end().line.0, regex_match.end().column.0),
        )
    }

    #[test]
    fn search_pattern_escapes_literals_and_sets_case_flag() {
        assert_eq!(search_pattern("1.5*(a)", false), r"(?i)1\.5\*\(a\)");
        assert_eq!(search_pattern("Foo", true), "(?-i)Foo");
    }

    #[test]
    fn find_matches_is_case_insensitive_by_default() {
        let (backend, _events) = TerminalBackend::new(TerminalSize::new(20, 5));
        backend.process_input(b"foo bar Foo\r\nfoofoo");

        let term = backend.term();
        let term = term.lock();

        let matches = find_matches(&term, "foo", false, MAX_SEARCH_MATCHES);
        assert_eq!(matches.len(), 4);
        assert_eq!(match_coords(&matches[0]), ((0, 0), (0, 2)));
        assert_eq!(match_coords(&matches[1]), ((0, 8), (0, 10)));
        assert_eq!(match_coords(&matches[2]), ((1, 0), (1, 2)));
        assert_eq!(match_coords(&matches[3]), ((1, 3), (1, 5)));

        let sensitive = find_matches(&term, "foo", true, MAX_SEARCH_MATCHES);
        assert_eq!(sensitive.len(), 3);
        assert_eq!(match_coords(&sensitive[0]), ((0, 0), (0, 2)));

        let upper = find_matches(&term, "Foo", true, MAX_SEARCH_MATCHES);
        assert_eq!(upper.len(), 1);
        assert_eq!(match_coords(&upper[0]), ((0, 8), (0, 10)));
    }

    #[test]
    fn find_matches_treats_query_as_literal_text() {
        let (backend, _events) = TerminalBackend::new(TerminalSize::new(20, 5));
        backend.process_input(b"125 and 1.5");

        let term = backend.term();
        let term = term.lock();

        let matches = find_matches(&term, "1.5", false, MAX_SEARCH_MATCHES);
        assert_eq!(matches.len(), 1);
        assert_eq!(match_coords(&matches[0]), ((0, 8), (0, 10)));
    }

    #[test]
    fn find_matches_searches_scrollback_history() {
        let (backend, _events) = TerminalBackend::new(TerminalSize::new(10, 3));
        for i in 0..10 {
            backend.process_input(format!("match {i}\r\n").as_bytes());
        }

        let term = backend.term();
        let term = term.lock();

        let matches = find_matches(&term, "match", false, MAX_SEARCH_MATCHES);
        assert_eq!(matches.len(), 10);
        // Ten lines printed with a trailing newline on a 3-line screen leaves
        // 8 lines in history; the earliest match starts at the top of it.
        assert_eq!(matches[0].start().line.0, -8);
        assert_eq!(matches[9].start().line.0, 1);
    }

    #[test]
    fn find_matches_respects_match_cap() {
        let (backend, _events) = TerminalBackend::new(TerminalSize::new(10, 3));
        for _ in 0..10 {
            backend.process_input(b"cap\r\n");
        }

        let term = backend.term();
        let term = term.lock();

        assert_eq!(find_matches(&term, "cap", false, 4).len(), 4);
    }

    #[test]
    fn find_matches_returns_empty_for_empty_query_or_no_hits() {
        let (backend, _events) = TerminalBackend::new(TerminalSize::new(10, 3));
        backend.process_input(b"hello");

        let term = backend.term();
        let term = term.lock();

        assert!(find_matches(&term, "", false, MAX_SEARCH_MATCHES).is_empty());
        assert!(find_matches(&term, "nothing", false, MAX_SEARCH_MATCHES).is_empty());
    }

    #[test]
    fn selection_wraps_around_both_directions() {
        let (backend, _events) = TerminalBackend::new(TerminalSize::new(20, 5));
        backend.process_input(b"one two one two one");

        let mut state = TerminalSearchState::default();
        let term = backend.term();
        {
            let term = term.lock();
            state.matches = find_matches(&term, "one", false, MAX_SEARCH_MATCHES);
        }
        assert_eq!(state.matches.len(), 3);

        // Forwards: 0 -> 1 -> 2 -> wraps to 0.
        assert_eq!(state.select_next(), Some(0));
        assert_eq!(state.select_next(), Some(1));
        assert_eq!(state.select_next(), Some(2));
        assert_eq!(state.select_next(), Some(0));

        // Backwards from 0 wraps to the last match.
        assert_eq!(state.select_previous(), Some(2));
        assert_eq!(state.select_previous(), Some(1));
    }

    #[test]
    fn selection_handles_empty_match_list() {
        let mut state = TerminalSearchState::default();
        assert_eq!(state.select_next(), None);
        assert_eq!(state.select_previous(), None);
        assert_eq!(state.current, None);
    }

    #[test]
    fn initial_match_index_prefers_nearest_match_above_viewport_bottom() {
        let (backend, _events) = TerminalBackend::new(TerminalSize::new(10, 3));
        for i in 0..10 {
            backend.process_input(format!("hit {i}\r\n").as_bytes());
        }

        let term = backend.term();
        let term = term.lock();
        let matches = find_matches(&term, "hit", false, MAX_SEARCH_MATCHES);

        // Viewport at the bottom (bottom grid line = screen_lines - 1 = 2):
        // pick the last match at or above it.
        assert_eq!(initial_match_index(&matches, 2), Some(9));
        // Scrolled all the way back (bottom grid line = -6): matches 0..=2
        // are at lines -8..=-6, so the nearest one above is index 2.
        assert_eq!(initial_match_index(&matches, -6), Some(2));
        // Bottom above every match start: fall back to the first match.
        assert_eq!(initial_match_index(&matches, -100), Some(0));
        assert_eq!(initial_match_index(&[], 0), None);
    }

    #[test]
    fn counter_label_reports_position_and_no_matches() {
        let mut state = TerminalSearchState::default();
        assert_eq!(state.counter_label(), None);

        state.query = "x".to_string();
        assert_eq!(state.counter_label(), Some("No matches".to_string()));

        state.matches = vec![
            Point::new(Line(0), Column(0))..=Point::new(Line(0), Column(0)),
            Point::new(Line(1), Column(0))..=Point::new(Line(1), Column(0)),
        ];
        state.current = Some(1);
        assert_eq!(state.counter_label(), Some("2/2".to_string()));
    }
}
