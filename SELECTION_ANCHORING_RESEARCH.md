# Selection Anchoring During Scroll - Research & Analysis

## Coordinate System Analysis

### Viewport-Relative Coordinates
The `pixel_to_cell()` function (line 178) returns **viewport-relative coordinates**:
```rust
let row = ((position.y - bounds.y) / self.cell_height()) as usize;
```
- `row 0` = top visible line in the viewport
- Coordinates are relative to the current visible area, not the absolute buffer position

## Scroll Behavior

### When Viewport Scrolls UP (positive scroll_lines, e.g., +1)
1. `term.scroll_display(Scroll::Delta(+1))` moves viewport UP in the scrollback buffer
2. Earlier content (from above) becomes visible
3. **Visual effect:** Content appears to slide DOWN on the screen
4. **Coordinate impact:** Text that was at viewport row 5 is now at viewport row 6
5. **Required adjustment:** Selection coordinates must INCREASE → `line + scroll_lines`

### When Viewport Scrolls DOWN (negative scroll_lines, e.g., -1)
1. `term.scroll_display(Scroll::Delta(-1))` moves viewport DOWN in the buffer
2. Later content (from below) becomes visible
3. **Visual effect:** Content appears to slide UP on the screen
4. **Coordinate impact:** Text that was at viewport row 5 is now at viewport row 4
5. **Required adjustment:** Selection coordinates must DECREASE → `line + (-1)` = `line - 1`

## The Bug

### Current (WRONG) Implementation
```rust
// When viewport scrolls up (positive delta), content moves down in screen space
// so we need to subtract from line coordinates to keep selection anchored
state.selection_start = Some((col, (line as i32 - scroll_lines).max(0) as usize));
```

**Problem:** The comment's logic is backwards!
- When content moves DOWN (scroll up), coordinates should INCREASE, not decrease
- When content moves UP (scroll down), coordinates should DECREASE, not increase
- Current code SUBTRACTS scroll_lines, giving the opposite effect

### Correct Implementation
```rust
// When viewport scrolls up (positive delta), content moves down in screen space
// so we need to ADD to line coordinates to keep selection anchored to the same content
state.selection_start = Some((col, (line as i32 + scroll_lines).max(0) as usize));
```

**Why this works:**
- Scroll UP (+1): `line + 1` → coordinates increase → selection follows content down ✓
- Scroll DOWN (-1): `line + (-1)` → coordinates decrease → selection follows content up ✓

## Conclusion
This is the same type of sign inversion bug as the previous scroll direction issue. The math is simply backwards - we need to ADD the scroll offset, not SUBTRACT it.
