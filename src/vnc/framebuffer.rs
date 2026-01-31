/// Framebuffer holding the current VNC screen contents (BGRA pixels)
#[derive(Debug)]
pub struct FrameBuffer {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
    pub dirty: Option<DirtyRect>,
}

#[derive(Debug, Clone, Copy)]
pub struct DirtyRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

pub(crate) fn rgba_to_bgra(mut data: Vec<u8>) -> Vec<u8> {
    for chunk in data.chunks_exact_mut(4) {
        chunk.swap(0, 2);
    }
    data
}

impl FrameBuffer {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            pixels: vec![0; (width * height * 4) as usize],
            dirty: Some(DirtyRect {
                x: 0,
                y: 0,
                width,
                height,
            }),
        }
    }

    fn mark_dirty(&mut self, x: u32, y: u32, w: u32, h: u32) {
        if w == 0 || h == 0 {
            return;
        }

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
                let x2 = (existing.x + existing.width).max(new_rect.x + new_rect.width);
                let y2 = (existing.y + existing.height).max(new_rect.y + new_rect.height);
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
        let stride = self.width as usize * 4;
        for row in 0..h as usize {
            let src_offset = row * w as usize * 4;
            let dst_offset = (y as usize + row) * stride + x as usize * 4;
            let len = w as usize * 4;
            if src_offset + len <= data.len() && dst_offset + len <= self.pixels.len() {
                self.pixels[dst_offset..dst_offset + len]
                    .copy_from_slice(&data[src_offset..src_offset + len]);
            }
        }
        self.mark_dirty(x, y, w, h);
    }

    /// Apply a 16-bit RGB565 image update to the framebuffer (little-endian unless big_endian is true)
    pub fn apply_raw_565(&mut self, x: u32, y: u32, w: u32, h: u32, data: &[u8], big_endian: bool) {
        let stride = self.width as usize * 4;
        let row_bytes = w as usize * 2;
        for row in 0..h as usize {
            let src_offset = row * row_bytes;
            let dst_offset = (y as usize + row) * stride + x as usize * 4;
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
        let stride = self.width as usize * 4;
        let mut temp = vec![0u8; w as usize * 4];
        // Iterate in reverse row order when dst overlaps src below,
        // to avoid overwriting source data before it's copied.
        let rows: Box<dyn Iterator<Item = usize>> = if dst_y > src_y {
            Box::new((0..h as usize).rev())
        } else {
            Box::new(0..h as usize)
        };
        for row in rows {
            let src_offset = (src_y as usize + row) * stride + src_x as usize * 4;
            let dst_offset = (dst_y as usize + row) * stride + dst_x as usize * 4;
            let len = w as usize * 4;
            if src_offset + len <= self.pixels.len() && dst_offset + len <= self.pixels.len() {
                temp[..len].copy_from_slice(&self.pixels[src_offset..src_offset + len]);
                self.pixels[dst_offset..dst_offset + len].copy_from_slice(&temp[..len]);
            }
        }
        self.mark_dirty(dst_x, dst_y, w, h);
    }

    /// Resize the framebuffer
    pub fn resize(&mut self, width: u32, height: u32) {
        self.width = width;
        self.height = height;
        self.pixels = vec![0; (width * height * 4) as usize];
        self.dirty = Some(DirtyRect {
            x: 0,
            y: 0,
            width,
            height,
        });
    }
}
