# Clipboard Copy Fix - Research Document

## Problem Statement
When selecting text in Portal and auto-scrolling the viewport, copying the selection results in **empty lines** for off-screen content instead of the actual text.

**Reproduction:**
1. User selects text spanning multiple lines
2. Auto-scroll moves viewport down
3. Copy operation → empty lines copied for the off-screen (scrollback) portion

## Root Cause Analysis

### Current Implementation (BROKEN)
Located in `src/terminal/widget.rs::get_selected_text()`:

```rust
fn get_selected_text(&self, start: (usize, i32), end: (usize, i32), cols: usize) -> String {
    let term = self.term.lock();
    let content = term.renderable_content();
    
    // Build grid from display_iter
    for indexed in content.display_iter {  // ❌ PROBLEM HERE
        let buffer_line = indexed.point.line.0;
        // ...
    }
}
```

**The Bug:** `content.display_iter` only iterates over **currently visible cells**. When selection includes scrollback content (off-screen above viewport), those cells aren't in `display_iter`, resulting in empty spaces in the grid.

### Alacritty's Approach (CORRECT)

From `alacritty_terminal/src/term/mod.rs`:

```rust
pub fn selection_to_string(&self) -> Option<String> {
    let selection_range = self.selection.as_ref().and_then(|s| s.to_range(self))?;
    let SelectionRange { start, end, .. } = selection_range;
    
    let mut res = String::new();
    
    // ... handle different selection types ...
    
    res = self.bounds_to_string(start, end);
    Some(res)
}

pub fn bounds_to_string(&self, start: Point, end: Point) -> String {
    let mut res = String::new();
    
    for line in (start.line.0..=end.line.0).map(Line::from) {
        let start_col = if line == start.line { start.column } else { Column(0) };
        let end_col = if line == end.line { end.column } else { self.last_column() };
        
        res += &self.line_to_string(line, start_col..end_col, line == end.line);
    }
    
    res.strip_suffix('\n').map(str::to_owned).unwrap_or(res)
}

fn line_to_string(&self, line: Line, mut cols: Range<Column>, include_wrapped_wide: bool) -> String {
    let mut text = String::new();
    
    let grid_line = &self.grid[line];  // ✅ DIRECT GRID ACCESS
    let line_length = cmp::min(grid_line.line_length(), cols.end + 1);
    
    // ... iterate through columns and extract characters ...
    
    for column in (cols.start.0..line_length.0).map(Column::from) {
        let cell = &grid_line[column];  // ✅ DIRECT CELL ACCESS
        
        if !cell.flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER) {
            text.push(cell.c);
            
            // Push zero-width characters
            for c in cell.zerowidth().into_iter().flatten() {
                text.push(*c);
            }
        }
    }
    
    // Handle line wrapping
    if cols.end >= self.columns() - 1
        && (line_length.0 == 0 || !self.grid[line][line_length - 1].flags.contains(Flags::WRAPLINE))
    {
        text.push('\n');
    }
    
    text
}
```

## Key Insights

### 1. Grid Structure (alacritty_terminal)

The Grid is a **ringbuffer** that stores all terminal content including scrollback:

- **Line Numbering:** Signed integers where:
  - `Line(0)` = bottommost visible line
  - `Line(-1), Line(-2), ...` = scrollback (above viewport)
  - Lines can be **negative** (scrollback content)

- **Grid Indexing:** Direct access via `term.grid()[line][column]`
  - The grid handles ringbuffer offset internally
  - Scrollback is always accessible (up to history limit)

- **Display Offset:** Viewport position relative to bottom
  - When `display_offset = 0`: viewing bottom (live output)
  - When `display_offset > 0`: scrolled back into history

### 2. Coordinate Systems

**Buffer Coordinates (absolute):**
- Line numbers relative to the buffer (can be negative)
- Independent of viewport position
- Used for selection storage
- Formula: `buffer_line = screen_line - display_offset`

**Screen Coordinates (viewport-relative):**
- Line numbers relative to visible viewport (always ≥ 0)
- Changes when user scrolls
- Used for rendering and mouse click detection
- Formula: `screen_line = buffer_line + display_offset`

### 3. The Fix Strategy

Instead of building a grid from `display_iter` (which only has visible cells), we should:

1. **Access the full grid directly:** `term.grid()`
2. **Iterate through buffer line range:** `start.line.0..=end.line.0`
3. **Use Line/Column types** for proper indexing
4. **Extract characters directly** from grid cells
5. **Handle line wrapping** using `WRAPLINE` flag

## Alacritty Terminal API

### Public Methods
```rust
// Access full terminal grid (includes scrollback)
pub fn grid(&self) -> &Grid<Cell>

// Grid dimensions
pub fn columns(&self) -> usize
pub fn last_column(&self) -> Column

// Renderable content (display_iter is for RENDERING ONLY, not text extraction)
pub fn renderable_content(&self) -> RenderableContent<'_>
```

### Grid Indexing
```rust
use alacritty_terminal::index::{Line, Column};

let grid = term.grid();
let cell = &grid[Line(-5)][Column(10)];  // Access scrollback
let character = cell.c;
let flags = cell.flags;
```

### Cell Flags
```rust
use alacritty_terminal::term::cell::Flags;

if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
    // Skip spacer cells
}

if cell.flags.contains(Flags::WRAPLINE) {
    // Line continues on next row (don't add \n)
}
```

## Implementation Plan

### Step 1: Rewrite `get_selected_text()`
Use direct grid access instead of `display_iter`:

```rust
fn get_selected_text(&self, start: (usize, i32), end: (usize, i32), cols: usize) -> String {
    let term = self.term.lock();
    let grid = term.grid();  // ✅ Get full grid
    
    let mut result = String::new();
    
    for buffer_line in start.1..=end.1 {
        let start_col = if buffer_line == start.1 { start.0 } else { 0 };
        let end_col = if buffer_line == end.1 { end.0 } else { cols.saturating_sub(1) };
        
        // Extract line text using Line type
        let line = Line(buffer_line);
        let line_text = extract_line_text(grid, line, start_col, end_col, cols);
        
        result.push_str(&line_text);
        
        // Add newline between lines (unless wrapped)
        if buffer_line < end.1 {
            result.push('\n');
        }
    }
    
    result
}
```

### Step 2: Implement Line Extraction
Handle wide chars, line wrapping, and trailing whitespace:

```rust
fn extract_line_text(
    grid: &Grid<Cell>,
    line: Line,
    start_col: usize,
    end_col: usize,
    cols: usize
) -> String {
    use alacritty_terminal::index::Column;
    use alacritty_terminal::term::cell::Flags;
    
    let grid_line = &grid[line];
    let mut text = String::new();
    
    for col_idx in start_col..=end_col.min(cols - 1) {
        let cell = &grid_line[Column(col_idx)];
        
        // Skip wide character spacers
        if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
            continue;
        }
        
        text.push(cell.c);
    }
    
    // Trim trailing whitespace from each line
    text.trim_end().to_string()
}
```

### Step 3: Test Cases

1. **Simple selection:** Single line, no scrolling
2. **Multi-line selection:** Across visible area
3. **Scrollback selection:** Select text, scroll down, copy (main bug)
4. **Wide characters:** CJK, emoji spanning 2 cells
5. **Wrapped lines:** Long lines that wrap (honor WRAPLINE flag)
6. **Empty lines:** Ensure blank lines copy correctly

## Verification

After implementation:
1. Run Portal
2. Generate scrollback content (e.g., `cat largefile.txt`)
3. Select text spanning multiple screenfulls
4. Let auto-scroll move viewport
5. Copy selection
6. Paste → should contain actual text, not empty lines

## References

- **Alacritty source:** `alacritty_terminal/src/term/mod.rs::selection_to_string()`
- **Grid structure:** `alacritty_terminal/src/grid/mod.rs`
- **Storage ringbuffer:** `alacritty_terminal/src/grid/storage.rs::compute_index()`
- **Portal dependencies:** `Cargo.toml` → `alacritty_terminal = "0.25.1"`

## Conclusion

The fix requires replacing the `display_iter` approach with direct grid access using `term.grid()`. The grid contains all scrollback content and can be indexed with negative Line numbers. This is exactly how Alacritty itself implements clipboard copy, so we should follow their proven approach.
