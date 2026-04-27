# Third-Party Notices

## Application runtime dependencies

Portal uses third-party packages as part of the application runtime. This
notice records the primary runtime packages that were previously surfaced in
the About dialog; redistribution must also comply with the complete dependency
license set in `Cargo.lock`.

| Package | Version | License | Project |
| --- | --- | --- | --- |
| Iced | 0.14.0 | MIT | https://github.com/iced-rs/iced |
| Alacritty Terminal | 0.26.0 | Apache-2.0 | https://github.com/alacritty/alacritty |
| Russh | 0.56.0 | Apache-2.0 | https://github.com/warp-tech/russh |
| vnc-rs | 0.5.2 | MIT OR Apache-2.0 | https://github.com/HsuJv/vnc-rs |
| Tokio | 1.52.1 | MIT | https://github.com/tokio-rs/tokio |

Vendored license texts for `vnc-rs` are included under `vendor/vnc-rs/`.

## Ghostty sprite renderer reference

Parts of Portal's built-in terminal sprite rendering are derived from the
Ghostty terminal emulator sprite renderer.

- Project: https://github.com/ghostty-org/ghostty
- Reference source: `src/font/sprite`
- Metrics/constraint reference source: `src/font/Metrics.zig`, `src/font/face.zig`,
  `src/font/nerd_font_attributes.zig`
- Reference commit: `b0d359cbbd945f9f3807327526ef79fcaf0477df`
- License: MIT

MIT License

Copyright (c) 2024 Mitchell Hashimoto, Ghostty contributors

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
