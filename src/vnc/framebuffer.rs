/// Framebuffer holding the current VNC screen contents (BGRA pixels)
#[derive(Debug)]
pub struct FrameBuffer {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
    pub dirty: Option<DirtyRect>,
    pub cursor_x: u32,
    pub cursor_y: u32,
    pub cursor_visible: bool,
    pub remote_cursor_seen: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct DirtyRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

const MAX_FRAMEBUFFER_BYTES: usize = 256 * 1024 * 1024;

pub(crate) fn rgba_to_bgra(mut data: Vec<u8>) -> Vec<u8> {
    for chunk in data.chunks_exact_mut(4) {
        chunk.swap(0, 2);
    }
    data
}

impl FrameBuffer {
    pub fn new(width: u32, height: u32) -> Self {
        let (width, height, pixels) = allocate_pixels(width, height);
        Self {
            width,
            height,
            pixels,
            dirty: None,
            cursor_x: 0,
            cursor_y: 0,
            cursor_visible: false,
            remote_cursor_seen: false,
        }
    }

    pub fn set_cursor_position(&mut self, x: u32, y: u32) {
        if self.width == 0 || self.height == 0 {
            return;
        }

        self.cursor_x = x.min(self.width - 1);
        self.cursor_y = y.min(self.height - 1);
        self.cursor_visible = true;
    }

    pub fn set_remote_cursor_seen(&mut self) {
        self.remote_cursor_seen = true;
    }

    fn mark_dirty(&mut self, x: u32, y: u32, w: u32, h: u32) {
        if w == 0 || h == 0 || x >= self.width || y >= self.height {
            return;
        }
        let w = w.min(self.width.saturating_sub(x));
        let h = h.min(self.height.saturating_sub(y));

        let new_rect = DirtyRect {
            x,
            y,
            width: w,
            height: h,
        };

        self.dirty = Some(match self.dirty {
            None => new_rect,
            Some(existing) => {
                let x1 = existing.x.min(new_rect.x);
                let y1 = existing.y.min(new_rect.y);
                let x2 = existing
                    .x
                    .saturating_add(existing.width)
                    .max(new_rect.x.saturating_add(new_rect.width));
                let y2 = existing
                    .y
                    .saturating_add(existing.height)
                    .max(new_rect.y.saturating_add(new_rect.height));
                DirtyRect {
                    x: x1,
                    y: y1,
                    width: x2.saturating_sub(x1),
                    height: y2.saturating_sub(y1),
                }
            }
        });
    }

    /// Apply a raw image update to the framebuffer
    pub fn apply_raw(&mut self, x: u32, y: u32, w: u32, h: u32, data: &[u8]) {
        let Some((w, h, _stride, len)) = self.clipped_region(x, y, w, h, 4) else {
            return;
        };
        for row in 0..h as usize {
            let Some(src_offset) = row.checked_mul(w as usize).and_then(|v| v.checked_mul(4))
            else {
                break;
            };
            let Some(dst_offset) = checked_pixel_offset(self.width, x, y + row as u32) else {
                break;
            };
            if src_offset + len <= data.len() && dst_offset + len <= self.pixels.len() {
                self.pixels[dst_offset..dst_offset + len]
                    .copy_from_slice(&data[src_offset..src_offset + len]);
            }
        }
        self.mark_dirty(x, y, w, h);
    }

    /// Apply a 16-bit RGB565 image update to the framebuffer (little-endian unless big_endian is true)
    pub fn apply_raw_565(&mut self, x: u32, y: u32, w: u32, h: u32, data: &[u8], big_endian: bool) {
        let Some((w, h, _stride, row_bytes)) = self.clipped_region(x, y, w, h, 2) else {
            return;
        };
        for row in 0..h as usize {
            let src_offset = row * row_bytes;
            let Some(dst_offset) = checked_pixel_offset(self.width, x, y + row as u32) else {
                break;
            };
            if src_offset + row_bytes > data.len() || dst_offset >= self.pixels.len() {
                break;
            }
            let mut dst = dst_offset;
            for col in 0..w as usize {
                let idx = src_offset + col * 2;
                if idx + 1 >= data.len() || dst + 4 > self.pixels.len() {
                    break;
                }
                let pixel = if big_endian {
                    u16::from_be_bytes([data[idx], data[idx + 1]])
                } else {
                    u16::from_le_bytes([data[idx], data[idx + 1]])
                };
                let r5 = (pixel >> 11) & 0x1f;
                let g6 = (pixel >> 5) & 0x3f;
                let b5 = pixel & 0x1f;
                let r8 = ((r5 << 3) | (r5 >> 2)) as u8;
                let g8 = ((g6 << 2) | (g6 >> 4)) as u8;
                let b8 = ((b5 << 3) | (b5 >> 2)) as u8;
                self.pixels[dst] = b8;
                self.pixels[dst + 1] = g8;
                self.pixels[dst + 2] = r8;
                self.pixels[dst + 3] = 255;
                dst += 4;
            }
        }
        self.mark_dirty(x, y, w, h);
    }

    /// Apply a copy rect operation
    pub fn apply_copy(&mut self, dst_x: u32, dst_y: u32, src_x: u32, src_y: u32, w: u32, h: u32) {
        let Some((w, h, _stride, len)) = self.clipped_copy_region(dst_x, dst_y, src_x, src_y, w, h)
        else {
            return;
        };
        let mut temp = vec![0u8; len];
        // Iterate in reverse row order when dst overlaps src below,
        // to avoid overwriting source data before it's copied.
        let rows: Box<dyn Iterator<Item = usize>> = if dst_y > src_y {
            Box::new((0..h as usize).rev())
        } else {
            Box::new(0..h as usize)
        };
        for row in rows {
            let Some(src_offset) = checked_pixel_offset(self.width, src_x, src_y + row as u32)
            else {
                break;
            };
            let Some(dst_offset) = checked_pixel_offset(self.width, dst_x, dst_y + row as u32)
            else {
                break;
            };
            if src_offset + len <= self.pixels.len() && dst_offset + len <= self.pixels.len() {
                temp[..len].copy_from_slice(&self.pixels[src_offset..src_offset + len]);
                self.pixels[dst_offset..dst_offset + len].copy_from_slice(&temp[..len]);
            }
        }
        self.mark_dirty(dst_x, dst_y, w, h);
    }

    /// Resize the framebuffer
    pub fn resize(&mut self, width: u32, height: u32) {
        let (width, height, pixels) = allocate_pixels(width, height);
        self.width = width;
        self.height = height;
        self.pixels = pixels;
        if width > 0 && height > 0 {
            self.cursor_x = self.cursor_x.min(width - 1);
            self.cursor_y = self.cursor_y.min(height - 1);
        } else {
            self.cursor_visible = false;
        }
        // Don't mark dirty here — the pixels are all black (zeroed) and
        // uploading them causes a black flash before real pixel data arrives.
        // The GPU texture will still be recreated on the next prepare() call
        // via the dimension mismatch check, and real pixel data from the
        // server will set the dirty flag when it arrives.
        self.dirty = None;
    }

    pub fn expected_pixel_len(&self) -> Option<usize> {
        pixel_buffer_len(self.width, self.height)
    }

    fn clipped_region(
        &self,
        x: u32,
        y: u32,
        w: u32,
        h: u32,
        bytes_per_source_pixel: usize,
    ) -> Option<(u32, u32, usize, usize)> {
        if x >= self.width || y >= self.height || w == 0 || h == 0 {
            return None;
        }
        let w = w.min(self.width.saturating_sub(x));
        let h = h.min(self.height.saturating_sub(y));
        let stride = (self.width as usize).checked_mul(4)?;
        let row_bytes = (w as usize).checked_mul(bytes_per_source_pixel)?;
        Some((w, h, stride, row_bytes))
    }

    fn clipped_copy_region(
        &self,
        dst_x: u32,
        dst_y: u32,
        src_x: u32,
        src_y: u32,
        w: u32,
        h: u32,
    ) -> Option<(u32, u32, usize, usize)> {
        if dst_x >= self.width
            || dst_y >= self.height
            || src_x >= self.width
            || src_y >= self.height
        {
            return None;
        }
        let w = w
            .min(self.width.saturating_sub(dst_x))
            .min(self.width.saturating_sub(src_x));
        let h = h
            .min(self.height.saturating_sub(dst_y))
            .min(self.height.saturating_sub(src_y));
        self.clipped_region(dst_x, dst_y, w, h, 4)
    }
}

fn pixel_buffer_len(width: u32, height: u32) -> Option<usize> {
    let len = (width as usize)
        .checked_mul(height as usize)?
        .checked_mul(4)?;
    (len <= MAX_FRAMEBUFFER_BYTES).then_some(len)
}

fn allocate_pixels(width: u32, height: u32) -> (u32, u32, Vec<u8>) {
    match pixel_buffer_len(width, height) {
        Some(len) => (width, height, vec![0; len]),
        None => (0, 0, Vec::new()),
    }
}

fn checked_pixel_offset(width: u32, x: u32, y: u32) -> Option<usize> {
    (y as usize)
        .checked_mul(width as usize)?
        .checked_add(x as usize)?
        .checked_mul(4)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dirty_rects_are_merged() {
        let mut fb = FrameBuffer::new(100, 100);

        fb.mark_dirty(10, 20, 5, 5);
        fb.mark_dirty(20, 10, 10, 30);

        let dirty = fb.dirty.unwrap();
        assert_eq!(dirty.x, 10);
        assert_eq!(dirty.y, 10);
        assert_eq!(dirty.width, 20);
        assert_eq!(dirty.height, 30);
    }

    #[test]
    fn cursor_position_is_clamped_to_framebuffer() {
        let mut fb = FrameBuffer::new(16, 9);

        fb.set_cursor_position(40, 20);

        assert_eq!(fb.cursor_x, 15);
        assert_eq!(fb.cursor_y, 8);
        assert!(fb.cursor_visible);
    }

    #[test]
    fn resize_clears_dirty_but_keeps_valid_cursor_state() {
        let mut fb = FrameBuffer::new(16, 9);
        fb.apply_raw(0, 0, 1, 1, &[1, 2, 3, 4]);
        fb.set_cursor_position(4, 5);

        fb.resize(8, 4);

        assert!(fb.dirty.is_none());
        assert_eq!(fb.pixels.len(), 8 * 4 * 4);
        assert_eq!(fb.cursor_x, 4);
        assert_eq!(fb.cursor_y, 3);
    }

    #[test]
    fn oversized_framebuffer_dimensions_are_rejected() {
        let fb = FrameBuffer::new(u32::MAX, u32::MAX);

        assert_eq!(fb.width, 0);
        assert_eq!(fb.height, 0);
        assert!(fb.pixels.is_empty());
    }

    #[test]
    fn raw_updates_outside_framebuffer_do_not_mark_dirty() {
        let mut fb = FrameBuffer::new(4, 4);

        fb.apply_raw(10, 10, 1, 1, &[1, 2, 3, 4]);

        assert!(fb.dirty.is_none());
    }

    #[test]
    fn raw_updates_are_clipped_to_framebuffer() {
        let mut fb = FrameBuffer::new(2, 2);

        fb.apply_raw(1, 1, 4, 4, &[1, 2, 3, 4]);

        assert_eq!(fb.dirty.unwrap().width, 1);
        assert_eq!(fb.dirty.unwrap().height, 1);
    }

    #[test]
    fn copy_rect_outside_framebuffer_does_not_mark_dirty() {
        let mut fb = FrameBuffer::new(4, 4);

        fb.apply_copy(0, 0, 10, 10, 1, 1);

        assert!(fb.dirty.is_none());
    }
}
