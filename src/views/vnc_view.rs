//! VNC viewer view
//!
//! Renders the VNC framebuffer with toolbar and special key dropdown.

use iced::widget::{button, column, container, pick_list, row, stack, text};
use iced::{Element, Fill};

use crate::app::managers::session_manager::VncActiveSession;
use crate::config::settings::VncScalingMode;
use crate::message::QualityLevel;
use crate::message::{Message, SessionId, VncMessage};
use crate::theme::{ScaledFonts, Theme};
use crate::vnc::widget::vnc_framebuffer_interactive;

/// Special key combinations available in the Send Keys dropdown
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SendKeyOption {
    CtrlAltDel,
    AltTab,
    Super,
    PrtSc,
    CtrlAltF1,
}

impl SendKeyOption {
    const ALL: &[SendKeyOption] = &[
        SendKeyOption::CtrlAltDel,
        SendKeyOption::AltTab,
        SendKeyOption::Super,
        SendKeyOption::PrtSc,
        SendKeyOption::CtrlAltF1,
    ];

    /// Return the keysyms for this key combination
    fn keysyms(self) -> Vec<u32> {
        match self {
            SendKeyOption::CtrlAltDel => vec![keysyms::CONTROL_L, keysyms::ALT_L, keysyms::DELETE],
            SendKeyOption::AltTab => vec![keysyms::ALT_L, keysyms::TAB],
            SendKeyOption::Super => vec![keysyms::SUPER_L],
            SendKeyOption::PrtSc => vec![keysyms::PRINT],
            SendKeyOption::CtrlAltF1 => vec![keysyms::CONTROL_L, keysyms::ALT_L, keysyms::F1],
        }
    }
}

impl std::fmt::Display for SendKeyOption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SendKeyOption::CtrlAltDel => write!(f, "Ctrl+Alt+Del"),
            SendKeyOption::AltTab => write!(f, "Alt+Tab"),
            SendKeyOption::Super => write!(f, "Super"),
            SendKeyOption::PrtSc => write!(f, "PrtSc"),
            SendKeyOption::CtrlAltF1 => write!(f, "Ctrl+Alt+F1"),
        }
    }
}

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

    // Single consolidated toolbar
    let send_keys_picker: Element<'a, Message> = {
        let sid = session_id;
        pick_list(
            SendKeyOption::ALL,
            None::<SendKeyOption>,
            move |option: SendKeyOption| {
                Message::Vnc(VncMessage::SendSpecialKeys {
                    session_id: sid,
                    keysyms: option.keysyms(),
                })
            },
        )
        .placeholder("Send Keys")
        .text_size(fonts.small)
        .padding([2, 6])
        .style(vnc_pick_list_style(theme))
        .menu_style(vnc_pick_list_menu_style(theme))
        .into()
    };

    let toolbar = container(
        row![
            // Status group: resolution, fps, scaling, quality
            text(resolution_text)
                .size(fonts.small)
                .color(theme.text_secondary),
            text(fps_text).size(fonts.small).color(fps_color),
            vnc_action_button(
                scaling_label,
                Message::Vnc(VncMessage::CycleScalingMode),
                theme,
                fonts
            ),
            {
                let (quality_label, quality_color) = match vnc.quality_level {
                    QualityLevel::High => ("● High", iced::Color::from_rgb8(0x40, 0xa0, 0x2b)),
                    QualityLevel::Medium => ("● Med", iced::Color::from_rgb8(0xdf, 0x8e, 0x1d)),
                    QualityLevel::Low => ("● Low", iced::Color::from_rgb8(0xd2, 0x0f, 0x39)),
                };
                text(quality_label).size(fonts.small).color(quality_color)
            },
            text("|").size(fonts.small).color(theme.text_muted),
            // Send Keys dropdown + Grab KB
            send_keys_picker,
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
            text("|").size(fonts.small).color(theme.text_muted),
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
        .spacing(6)
        .align_y(iced::Alignment::Center),
    )
    .padding([3, 12])
    .width(Fill)
    .style(move |_| iced::widget::container::Style {
        background: Some(theme.surface.into()),
        ..Default::default()
    });

    // Framebuffer — custom shader widget with mouse event handling
    let fb_content: Element<'a, Message> = vnc_framebuffer_interactive(
        &vnc.session.framebuffer,
        scaling_mode,
        session_id,
        fb_width,
        fb_height,
    );

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
        column![toolbar, framebuffer]
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

/// Pick list style for VNC toolbar Send Keys dropdown
fn vnc_pick_list_style(
    theme: Theme,
) -> impl Fn(&iced::Theme, pick_list::Status) -> pick_list::Style {
    move |_t, status| {
        let bg = match status {
            pick_list::Status::Hovered | pick_list::Status::Opened { .. } => theme.hover,
            _ => theme.surface,
        };
        pick_list::Style {
            background: bg.into(),
            text_color: theme.text_secondary,
            placeholder_color: theme.text_secondary,
            handle_color: theme.text_muted,
            border: iced::Border {
                radius: 3.0.into(),
                width: 1.0,
                color: theme.border,
            },
        }
    }
}

/// Menu style for VNC toolbar Send Keys dropdown
fn vnc_pick_list_menu_style(theme: Theme) -> impl Fn(&iced::Theme) -> iced::overlay::menu::Style {
    move |_t| iced::overlay::menu::Style {
        background: theme.surface.into(),
        text_color: theme.text_primary,
        selected_text_color: theme.text_primary,
        selected_background: theme.accent.into(),
        border: iced::Border {
            radius: 4.0.into(),
            width: 1.0,
            color: theme.border,
        },
        shadow: iced::Shadow::default(),
    }
}
