# Selection Anchoring Logic Verification

## Fixed Implementation
```rust
// Compensate selection endpoints for scroll offset
// When viewport scrolls up (positive delta), content moves down in screen space
// so we need to ADD to line coordinates to keep selection anchored to the same content
if let Some((col, line)) = state.selection_start {
    state.selection_start = Some((col, (line as i32 + scroll_lines).max(0) as usize));
}
if let Some((col, line)) = state.selection_end {
    state.selection_end = Some((col, (line as i32 + scroll_lines).max(0) as usize));
}
```

## Verification Test Cases

### Test Case 1: Scrolling UP (near top edge, dragging selection upward)
**Initial State:**
- Mouse near top edge, triggering auto-scroll
- Selection at viewport row 5
- scroll_lines = +1 (positive, scrolling viewport up)

**What Happens:**
1. Viewport scrolls UP in buffer (earlier content becomes visible)
2. Content visually slides DOWN on screen
3. Text that was at row 5 is now at row 6 (moved down)
4. Selection adjustment: `5 + 1 = 6` ✓

**Result:** Selection stays anchored to the same text content ✓

### Test Case 2: Scrolling DOWN (near bottom edge, dragging selection downward)
**Initial State:**
- Mouse near bottom edge, triggering auto-scroll
- Selection at viewport row 15
- scroll_lines = -1 (negative, scrolling viewport down)

**What Happens:**
1. Viewport scrolls DOWN in buffer (later content becomes visible)
2. Content visually slides UP on screen
3. Text that was at row 15 is now at row 14 (moved up)
4. Selection adjustment: `15 + (-1) = 14` ✓

**Result:** Selection stays anchored to the same text content ✓

### Test Case 3: Fast Scrolling UP (near top edge, far from border)
**Initial State:**
- Mouse very close to top edge
- Selection at viewport row 10
- scroll_lines = +3 (fast scroll up)

**What Happens:**
1. Viewport scrolls UP 3 lines
2. Content slides DOWN 3 rows
3. Text at row 10 is now at row 13
4. Selection adjustment: `10 + 3 = 13` ✓

**Result:** Selection stays anchored to the same text content ✓

### Test Case 4: Boundary Protection
**Initial State:**
- Selection at viewport row 1
- scroll_lines = -2 (scroll down 2 lines)

**What Happens:**
1. Calculation: `1 + (-2) = -1`
2. `.max(0)` clamps to 0
3. Selection moves to row 0 (top of viewport) ✓

**Result:** Selection doesn't underflow, clamped safely ✓

## Coordinate System Summary
- **Type:** Viewport-relative coordinates
- **Origin:** Row 0 = top of visible viewport (not buffer)
- **Scroll UP (+):** Content moves DOWN → coordinates INCREASE
- **Scroll DOWN (-):** Content moves UP → coordinates DECREASE
- **Formula:** `new_line = old_line + scroll_lines`

## Comparison with Previous Bug
Both bugs had the same root cause: **inverted sign**
- **Scroll direction bug:** Signs on scroll_lines calculation were backwards
- **Selection anchoring bug:** Sign on coordinate adjustment was backwards

Both required changing a minus to a plus to fix the inversion.
