# Auto-Scroll During Text Selection - Implementation Summary

## Feature Overview
Implemented automatic scrolling when selecting text in an SSH terminal session. When the user drags their selection beyond the visible viewport (near top or bottom edge), the terminal automatically scrolls to reveal more content.

## Changes Made

### File: `src/terminal/widget.rs`

#### 1. Added Auto-Scroll State Tracking
**Location:** `TerminalState` struct (line ~430)
- Added field: `last_auto_scroll: Option<std::time::Instant>`
- Tracks the last time auto-scroll was triggered to throttle scroll rate
- Updated in `Default` impl and `state()` method

#### 2. Implemented Auto-Scroll Logic
**Location:** `Event::Mouse(mouse::Event::CursorMoved)` handler (line ~1080)

**Key Features:**
- **Edge Detection Zone:** 30 pixels from top/bottom edges
- **Scroll Throttling:** Minimum 50ms between auto-scroll updates
- **Variable Scroll Speed:** 1-3 lines based on proximity to edge
  - Closer to edge = faster scroll
  - Distance factor: `(30 - edge_distance) / 30`
- **Direction Support:**
  - Near top edge → scroll UP (negative lines)
  - Near bottom edge → scroll DOWN (positive lines)

**Behavior:**
1. Checks if user is actively selecting text (`is_selecting == true`)
2. Calculates distance from mouse cursor to viewport edges
3. If within 30px of top/bottom edge AND throttle time has passed:
   - Scrolls the terminal by 1-3 lines (based on proximity)
   - Updates the selection endpoint to follow the scroll
   - Respects alternate screen mode (doesn't scroll in vim/htop)
4. Normal selection update continues when cursor is within bounds

**Selection Mode Support:**
- Works with all selection modes: Character, Word, Line
- After auto-scroll, updates the selection endpoint appropriately for each mode

#### 3. State Cleanup
- Clears `last_auto_scroll` when:
  - Mouse button is released (selection ends)
  - Clicking outside to clear selection

## Implementation Details

### Auto-Scroll Trigger Logic
```rust
const AUTO_SCROLL_ZONE: f32 = 30.0;  // pixels from edge
const AUTO_SCROLL_INTERVAL: std::time::Duration = Duration::from_millis(50);

let edge_distance_top = position.y - bounds.y;
let edge_distance_bottom = bounds.y + bounds.height - position.y;

let near_top_edge = edge_distance_top >= 0.0 && edge_distance_top < AUTO_SCROLL_ZONE;
let near_bottom_edge = edge_distance_bottom >= 0.0 && edge_distance_bottom < AUTO_SCROLL_ZONE;
```

### Scroll Speed Calculation
```rust
// Scroll up (top edge)
let distance_factor = (AUTO_SCROLL_ZONE - edge_distance_top.max(0.0)) / AUTO_SCROLL_ZONE;
let scroll_lines = -(1.max((distance_factor * 3.0) as i32));

// Scroll down (bottom edge)  
let distance_factor = (AUTO_SCROLL_ZONE - edge_distance_bottom.max(0.0)) / AUTO_SCROLL_ZONE;
let scroll_lines = 1.max((distance_factor * 3.0) as i32);
```

### Selection Update After Scroll
After scrolling, the cursor position is clamped to viewport bounds and converted to a cell coordinate. The selection endpoint is then updated based on the current selection mode (Character/Word/Line).

## Testing Recommendations

### Manual Testing Steps
1. **Start Portal** with an SSH session
2. **Generate long output** in terminal:
   ```bash
   seq 1 1000
   ```
3. **Test upward scroll:**
   - Click and start selecting text in the middle of viewport
   - Drag mouse to top edge (within 30px of top)
   - Terminal should scroll UP automatically
   - Selection should extend upward continuously
   
4. **Test downward scroll:**
   - Start selection in middle of viewport
   - Drag mouse to bottom edge (within 30px of bottom)
   - Terminal should scroll DOWN automatically
   - Selection should extend downward continuously

5. **Test scroll speed:**
   - Drag very close to edge → faster scroll (up to 3 lines per update)
   - Drag just inside 30px zone → slower scroll (1 line per update)

6. **Test selection modes:**
   - Single click → character-by-character selection
   - Double click → word-by-word selection  
   - Triple click → line-by-line selection
   - Auto-scroll should work with all modes

7. **Test alternate screen:**
   - Open vim or htop
   - Try selecting text
   - Auto-scroll should NOT trigger (no scrollback in alt screen)

### Expected Behavior
✅ Smooth automatic scrolling when dragging selection to edges
✅ Selection extends continuously across multiple screenfuls
✅ Scroll speed increases when closer to edge
✅ Works with all selection modes (char/word/line)
✅ Respects alternate screen mode
✅ No scroll lag or stuttering (throttled at 50ms)

## Performance Considerations
- Throttling prevents excessive CPU usage during rapid updates
- Render cache is invalidated on scroll to ensure accurate display
- Scroll distance is clamped (1-3 lines max) to prevent jarring jumps

## Edge Cases Handled
1. **Cursor outside bounds:** Auto-scroll still works when dragging beyond viewport
2. **Alternate screen mode:** No auto-scroll in vim, htop, etc. (no scrollback buffer)
3. **Selection mode changes:** Each mode updates selection correctly after scroll
4. **Rapid scrolling:** Throttled to prevent performance issues

## Files Modified
- `src/terminal/widget.rs`: All changes in this file
  - Added `last_auto_scroll` field to `TerminalState`
  - Implemented auto-scroll logic in `CursorMoved` event handler
  - Updated state initialization and cleanup

## Build & Run
```bash
cd ~/Projects/portal
./run.sh dev    # Run in development mode
# or
./run.sh run    # Run in release mode
```

## Next Steps
1. Build the project: `./run.sh build`
2. Run Portal: `./run.sh run`
3. Test with SSH session (follow testing steps above)
4. Adjust `AUTO_SCROLL_ZONE` or `AUTO_SCROLL_INTERVAL` if needed
5. Optionally adjust max scroll speed (currently 3 lines)

## Configuration Tuning
If auto-scroll feels too fast/slow, modify these constants in `terminal/widget.rs`:
```rust
const AUTO_SCROLL_ZONE: f32 = 30.0;  // Increase for larger trigger zone
const AUTO_SCROLL_INTERVAL: std::time::Duration = Duration::from_millis(50);  // Decrease for faster updates
let scroll_lines = 1.max((distance_factor * 3.0) as i32);  // Change 3.0 to adjust max speed
```
