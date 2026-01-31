use vnc::{PixelFormat, VncEncoding};

use crate::config::settings::VncEncodingPreference;

pub fn build_encodings(
    preference: VncEncodingPreference,
    is_private: bool,
    allow_tight: bool,
    include_cursor: bool,
) -> Vec<VncEncoding> {
    let mut encodings = Vec::new();

    // Pseudo-encodings for better behavior (cursor + desktop size updates)
    if include_cursor {
        encodings.push(VncEncoding::CursorPseudo);
    }
    encodings.push(VncEncoding::DesktopSizePseudo);

    let mut order = match preference {
        VncEncodingPreference::Auto => {
            if is_private {
                vec![
                    VncEncoding::Zrle,
                    VncEncoding::Tight,
                    VncEncoding::CopyRect,
                    VncEncoding::Raw,
                ]
            } else {
                vec![
                    VncEncoding::Tight,
                    VncEncoding::Zrle,
                    VncEncoding::CopyRect,
                    VncEncoding::Raw,
                ]
            }
        }
        VncEncodingPreference::Tight => vec![
            VncEncoding::Tight,
            VncEncoding::Zrle,
            VncEncoding::CopyRect,
            VncEncoding::Raw,
        ],
        VncEncodingPreference::Zrle => vec![
            VncEncoding::Zrle,
            VncEncoding::Tight,
            VncEncoding::CopyRect,
            VncEncoding::Raw,
        ],
        VncEncodingPreference::Raw => vec![
            VncEncoding::Raw,
            VncEncoding::Zrle,
            VncEncoding::Tight,
            VncEncoding::CopyRect,
        ],
    };

    if !allow_tight {
        order.retain(|encoding| *encoding != VncEncoding::Tight);
    }

    for encoding in order {
        if !encodings.contains(&encoding) {
            encodings.push(encoding);
        }
    }

    if !encodings.contains(&VncEncoding::Raw) {
        encodings.push(VncEncoding::Raw);
    }

    encodings
}

pub fn pixel_format_from_depth(depth: u8) -> PixelFormat {
    match depth {
        16 => {
            let mut pf = PixelFormat::bgra();
            pf.bits_per_pixel = 16;
            pf.depth = 16;
            pf.big_endian_flag = 0;
            pf.true_color_flag = 1;
            pf.red_max = 31;
            pf.green_max = 63;
            pf.blue_max = 31;
            pf.red_shift = 11;
            pf.green_shift = 5;
            pf.blue_shift = 0;
            pf
        }
        _ => PixelFormat::bgra(),
    }
}
