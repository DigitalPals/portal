//! VNC viewer view
//!
//! Renders the VNC framebuffer with toolbar, status bar, and special key buttons.

use iced::widget::{button, column, container, row, stack, text};
use iced::{Element, Fill};

use crate::app::managers::session_manager::VncActiveSession;
use crate::config::settings::VncScalingMode;
use crate::message::{Message, SessionId, VncMessage};
use crate::theme::{ScaledFonts, Theme};
use crate::message::QualityLevel;
use crate::vnc::widget::vnc_framebuffer_interactive;

/// X11 keysym constants for special keys
mod keysyms {
    pub const CONTROL_L: u32 = 0xffe3;
    pub const ALT_L: u32 = 0xffe9;
    pub const DELETE: u32 = 0xffff;
    pub const TAB: u32 = 0xff09;
    pub const SUPER_L: u32 = 0xffeb;
    pub const PRINT: u32 = 0xff61;
    pub const F1: u32 = 0xffbe;
}

/// Build the VNC viewer view with toolbar, special keys, and framebuffer display
pub fn vnc_viewer_view<'a>(
    session_id: SessionId,
    vnc: &'a VncActiveSession,
    theme: Theme,
    fonts: ScaledFonts,
    scaling_mode: VncScalingMode,
) -> Element<'a, Message> {
    let fb = vnc.session.framebuffer.lock();
    let resolution_text = format!("{}x{}", fb.width, fb.height);
    let fb_width = fb.width;
    let fb_height = fb.height;
    drop(fb);

    let is_fullscreen = vnc.fullscreen;

    // Scaling mode label
    let scaling_label = match scaling_mode {
        VncScalingMode::Fit => "Fit",
        VncScalingMode::Actual => "1:1",
        VncScalingMode::Stretch => "Stretch",
    };

    // FPS indicator with color coding
    let fps = vnc.current_fps;
    let fps_color = if fps >= 20.0 {
        iced::Color::from_rgb8(0x40, 0xa0, 0x2b) // green
    } else if fps >= 10.0 {
        iced::Color::from_rgb8(0xdf, 0x8e, 0x1d) // yellow
    } else {
        iced::Color::from_rgb8(0xd2, 0x0f, 0x39) // red
    };
    let fps_text = format!("{:.0} fps", fps);

    // Status bar
    let status_bar = container(
        row![
            text("VNC").size(fonts.label).color(theme.text_secondary),
            text(" | ").size(fonts.label).color(theme.text_muted),
            text(&vnc.host_name)
                .size(fonts.label)
                .color(theme.text_primary),
            text(" | ").size(fonts.label).color(theme.text_muted),
            text(resolution_text)
                .size(fonts.label)
                .color(theme.text_secondary),
            text(" | ").size(fonts.label).color(theme.text_muted),
            text(fps_text).size(fonts.label).color(fps_color),
            text(" | ").size(fonts.label).color(theme.text_muted),
            vnc_status_button(
                scaling_label,
                Message::Vnc(VncMessage::CycleScalingMode),
                theme,
                fonts
            ),
            text(" | ").size(fonts.label).color(theme.text_muted),
            {
                let (quality_label, quality_color) = match vnc.quality_level {
                    QualityLevel::High => ("● High", iced::Color::from_rgb8(0x40, 0xa0, 0x2b)),
                    QualityLevel::Medium => ("● Med", iced::Color::from_rgb8(0xdf, 0x8e, 0x1d)),
                    QualityLevel::Low => ("● Low", iced::Color::from_rgb8(0xd2, 0x0f, 0x39)),
                };
                text(quality_label).size(fonts.label).color(quality_color)
            },
        ]
        .spacing(4)
        .align_y(iced::Alignment::Center),
    )
    .padding([4, 12])
    .width(Fill)
    .style(move |_| iced::widget::container::Style {
        background: Some(theme.surface.into()),
        ..Default::default()
    });

    // Special keys toolbar
    let special_keys_bar = container(
        row![
            vnc_action_button(
                "Ctrl+Alt+Del",
                Message::Vnc(VncMessage::SendSpecialKeys {
                    session_id,
                    keysyms: vec![keysyms::CONTROL_L, keysyms::ALT_L, keysyms::DELETE],
                }),
                theme,
                fonts,
            ),
            vnc_action_button(
                "Alt+Tab",
                Message::Vnc(VncMessage::SendSpecialKeys {
                    session_id,
                    keysyms: vec![keysyms::ALT_L, keysyms::TAB],
                }),
                theme,
                fonts,
            ),
            vnc_action_button(
                "Super",
                Message::Vnc(VncMessage::SendSpecialKeys {
                    session_id,
                    keysyms: vec![keysyms::SUPER_L],
                }),
                theme,
                fonts,
            ),
            vnc_action_button(
                "PrtSc",
                Message::Vnc(VncMessage::SendSpecialKeys {
                    session_id,
                    keysyms: vec![keysyms::PRINT],
                }),
                theme,
                fonts,
            ),
            vnc_action_button(
                "Ctrl+Alt+F1",
                Message::Vnc(VncMessage::SendSpecialKeys {
                    session_id,
                    keysyms: vec![keysyms::CONTROL_L, keysyms::ALT_L, keysyms::F1],
                }),
                theme,
                fonts,
            ),
            vnc_action_button(
                if vnc.keyboard_passthrough {
                    "Release KB"
                } else {
                    "Grab KB"
                },
                Message::Vnc(VncMessage::ToggleKeyboardPassthrough),
                theme,
                fonts,
            ),
            // Spacer
            iced::widget::Space::new().width(Fill),
            // Monitor selector (only if multiple monitors detected)
            {
                let monitor_buttons: Vec<Element<'a, Message>> = if vnc.monitors.len() > 1 {
                    let mut btns = vec![vnc_action_button(
                        "All",
                        Message::Vnc(VncMessage::SelectMonitor(session_id, None)),
                        theme,
                        fonts,
                    )];
                    for (i, _screen) in vnc.monitors.iter().enumerate() {
                        let label_str: String = format!("Mon {}", i + 1);
                        btns.push(vnc_action_button_owned(
                            label_str,
                            Message::Vnc(VncMessage::SelectMonitor(session_id, Some(i))),
                            theme,
                            fonts,
                        ));
                    }
                    btns
                } else {
                    vec![]
                };
                row(monitor_buttons).spacing(2)
            },
            vnc_action_button(
                "Screenshot",
                Message::Vnc(VncMessage::CaptureScreenshot(session_id)),
                theme,
                fonts,
            ),
            vnc_action_button(
                if is_fullscreen {
                    "Exit Fullscreen"
                } else {
                    "Fullscreen"
                },
                Message::Vnc(VncMessage::ToggleFullscreen),
                theme,
                fonts,
            ),
        ]
        .spacing(4)
        .align_y(iced::Alignment::Center),
    )
    .padding([2, 12])
    .width(Fill)
    .style(move |_| iced::widget::container::Style {
        background: Some(theme.surface.into()),
        ..Default::default()
    });

    // Framebuffer — custom shader widget with mouse event handling
    let fb_content: Element<'a, Message> =
        vnc_framebuffer_interactive(&vnc.session.framebuffer, scaling_mode, session_id, fb_width, fb_height);

    let framebuffer = container(fb_content)
        .width(Fill)
        .height(Fill)
        .align_x(iced::Alignment::Center)
        .align_y(iced::Alignment::Center)
        .style(move |_| iced::widget::container::Style {
            background: Some(iced::Color::BLACK.into()),
            ..Default::default()
        });

    if is_fullscreen {
        // In fullscreen, just show framebuffer with a small overlay hint
        let hint = container(
            text("F11 to exit fullscreen")
                .size(fonts.small)
                .color(iced::Color::from_rgba8(255, 255, 255, 0.5)),
        )
        .padding([2, 8])
        .style(move |_| iced::widget::container::Style {
            background: Some(iced::Color::from_rgba8(0, 0, 0, 0.4).into()),
            border: iced::Border {
                radius: 4.0.into(),
                ..Default::default()
            },
            ..Default::default()
        });

        stack![
            framebuffer,
            container(hint)
                .width(Fill)
                .align_x(iced::Alignment::Center)
                .padding(8),
        ]
        .width(Fill)
        .height(Fill)
        .into()
    } else {
        column![status_bar, special_keys_bar, framebuffer]
            .width(Fill)
            .height(Fill)
            .into()
    }
}

/// Small toolbar button for VNC actions
fn vnc_action_button<'a>(
    label: &'a str,
    on_press: Message,
    theme: Theme,
    fonts: ScaledFonts,
) -> Element<'a, Message> {
    button(text(label).size(fonts.small).color(theme.text_secondary))
        .on_press(on_press)
        .padding([2, 6])
        .style(move |_t, status| {
            let bg = match status {
                button::Status::Hovered => theme.hover,
                _ => theme.surface,
            };
            button::Style {
                background: Some(bg.into()),
                border: iced::Border {
                    radius: 3.0.into(),
                    width: 1.0,
                    color: theme.border,
                },
                text_color: theme.text_secondary,
                ..Default::default()
            }
        })
        .into()
}

/// Small toolbar button with owned label string
fn vnc_action_button_owned<'a>(
    label: String,
    on_press: Message,
    theme: Theme,
    fonts: ScaledFonts,
) -> Element<'a, Message> {
    button(text(label).size(fonts.small).color(theme.text_secondary))
        .on_press(on_press)
        .padding([2, 6])
        .style(move |_t, status| {
            let bg = match status {
                button::Status::Hovered => theme.hover,
                _ => theme.surface,
            };
            button::Style {
                background: Some(bg.into()),
                border: iced::Border {
                    radius: 3.0.into(),
                    width: 1.0,
                    color: theme.border,
                },
                text_color: theme.text_secondary,
                ..Default::default()
            }
        })
        .into()
}

/// Small status bar button (for scaling mode toggle etc.)
fn vnc_status_button<'a>(
    label: &'a str,
    on_press: Message,
    theme: Theme,
    fonts: ScaledFonts,
) -> Element<'a, Message> {
    button(text(label).size(fonts.label).color(theme.text_secondary))
        .on_press(on_press)
        .padding([1, 6])
        .style(move |_t, status| {
            let bg = match status {
                button::Status::Hovered => theme.hover,
                _ => iced::Color::TRANSPARENT,
            };
            button::Style {
                background: Some(bg.into()),
                border: iced::Border {
                    radius: 3.0.into(),
                    width: 1.0,
                    color: theme.border,
                },
                text_color: theme.text_secondary,
                ..Default::default()
            }
        })
        .into()
}
