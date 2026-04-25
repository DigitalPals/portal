//! VNC viewer view
//!
//! Renders the VNC framebuffer with toolbar and special key dropdown.

use iced::widget::{button, column, container, pick_list, row, stack, text};
use iced::{Element, Fill, Length};

use crate::app::managers::session_manager::VncActiveSession;
use crate::config::settings::{VncQualityPreset, VncScalingMode};
use crate::message::{Message, SessionId, VncMessage};
use crate::theme::{ScaledFonts, Theme};
use crate::vnc::session::VncStatsSnapshot;
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
    quality_preset: VncQualityPreset,
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

    let stats = vnc.session.stats_snapshot();

    let fps_text = format_fps(vnc.current_fps, stats.first_frame_received);
    let update_age_text = format_update_age(&stats);
    let encoding_text = stats
        .last_update_kind
        .map(|kind| kind.label())
        .unwrap_or("pending");

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
            vnc_metric_label(resolution_text, 82.0, theme.text_secondary, theme, fonts),
            vnc_metric_label(fps_text, 56.0, theme.text_secondary, theme, fonts),
            vnc_metric_label(update_age_text, 58.0, theme.text_muted, theme, fonts),
            vnc_metric_label(
                encoding_text.to_string(),
                54.0,
                theme.text_muted,
                theme,
                fonts
            ),
            vnc_metric_label(
                vnc.status_text.clone(),
                96.0,
                theme.text_muted,
                theme,
                fonts
            ),
            vnc_action_button_sized(
                scaling_label,
                Message::Vnc(VncMessage::CycleScalingMode),
                54.0,
                theme,
                fonts
            ),
            vnc_action_button_sized(
                quality_preset.label(),
                Message::Vnc(VncMessage::CycleQualityPreset),
                68.0,
                theme,
                fonts
            ),
            text("|").size(fonts.small).color(theme.text_muted),
            // Send Keys dropdown + Grab KB
            send_keys_picker,
            vnc_action_button_sized(
                if vnc.keyboard_passthrough {
                    "Release KB"
                } else {
                    "Grab KB"
                },
                Message::Vnc(VncMessage::ToggleKeyboardPassthrough),
                78.0,
                theme,
                fonts,
            ),
            vnc_action_button_sized(
                if vnc.view_only {
                    "View Only"
                } else {
                    "Interactive"
                },
                Message::Vnc(VncMessage::ToggleViewOnly),
                82.0,
                theme,
                fonts,
            ),
            vnc_action_button_sized(
                if vnc.show_cursor_dot {
                    "Cursor Dot"
                } else {
                    "No Cursor Dot"
                },
                Message::Vnc(VncMessage::ToggleCursorDot),
                94.0,
                theme,
                fonts,
            ),
            text("|").size(fonts.small).color(theme.text_muted),
            // Spacer
            iced::widget::Space::new().width(Fill),
            vnc_action_button(
                "Refresh",
                Message::Vnc(VncMessage::ManualRefresh(session_id)),
                theme,
                fonts,
            ),
            vnc_action_button_sized(
                if vnc.show_stats_overlay {
                    "Hide Stats"
                } else {
                    "Stats"
                },
                Message::Vnc(VncMessage::ToggleStatsOverlay),
                70.0,
                theme,
                fonts,
            ),
            vnc_action_button(
                "Screenshot",
                Message::Vnc(VncMessage::CaptureScreenshot(session_id)),
                theme,
                fonts,
            ),
            vnc_action_button_sized(
                if is_fullscreen {
                    "Exit Fullscreen"
                } else {
                    "Fullscreen"
                },
                Message::Vnc(VncMessage::ToggleFullscreen),
                104.0,
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
        vnc.show_cursor_dot,
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

    let waiting_overlay: Option<Element<'a, Message>> = if !stats.first_frame_received {
        Some(
            container(
                text("Waiting for first VNC frame")
                    .size(fonts.body)
                    .color(iced::Color::from_rgba8(255, 255, 255, 0.8)),
            )
            .padding([6, 12])
            .style(move |_| iced::widget::container::Style {
                background: Some(iced::Color::from_rgba8(0, 0, 0, 0.55).into()),
                border: iced::Border {
                    radius: 4.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            })
            .into(),
        )
    } else {
        None
    };

    let stats_overlay: Option<Element<'a, Message>> = if vnc.show_stats_overlay {
        let rect_text = stats
            .last_update_rect
            .map(|(x, y, w, h)| format!("rect {}x{}+{}+{}", w, h, x, y))
            .unwrap_or_else(|| "rect pending".to_string());
        let bytes_text = format!("{} KB", stats.bytes_total / 1024);
        Some(
            container(
                column![
                    text(format!("updates {}", stats.updates_total))
                        .size(fonts.small)
                        .color(iced::Color::WHITE),
                    text(format!("pixels {}", stats.pixels_total))
                        .size(fonts.small)
                        .color(iced::Color::WHITE),
                    text(bytes_text).size(fonts.small).color(iced::Color::WHITE),
                    text(rect_text).size(fonts.small).color(iced::Color::WHITE),
                    text(format!("cursor updates {}", stats.cursor_updates))
                        .size(fonts.small)
                        .color(iced::Color::WHITE),
                ]
                .spacing(2),
            )
            .padding([6, 8])
            .style(move |_| iced::widget::container::Style {
                background: Some(iced::Color::from_rgba8(0, 0, 0, 0.55).into()),
                border: iced::Border {
                    radius: 4.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            })
            .into(),
        )
    } else {
        None
    };

    let framebuffer_stack: Element<'a, Message> = match (waiting_overlay, stats_overlay) {
        (Some(waiting), Some(stats)) => stack![
            framebuffer,
            container(waiting)
                .width(Fill)
                .height(Fill)
                .align_x(iced::Alignment::Center)
                .align_y(iced::Alignment::Center),
            container(stats)
                .width(Fill)
                .height(Fill)
                .align_x(iced::Alignment::Start)
                .align_y(iced::Alignment::Start)
                .padding(10),
        ]
        .width(Fill)
        .height(Fill)
        .into(),
        (Some(waiting), None) => stack![
            framebuffer,
            container(waiting)
                .width(Fill)
                .height(Fill)
                .align_x(iced::Alignment::Center)
                .align_y(iced::Alignment::Center),
        ]
        .width(Fill)
        .height(Fill)
        .into(),
        (None, Some(stats)) => stack![
            framebuffer,
            container(stats)
                .width(Fill)
                .height(Fill)
                .align_x(iced::Alignment::Start)
                .align_y(iced::Alignment::Start)
                .padding(10),
        ]
        .width(Fill)
        .height(Fill)
        .into(),
        (None, None) => framebuffer.into(),
    };

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
            framebuffer_stack,
            container(hint)
                .width(Fill)
                .align_x(iced::Alignment::Center)
                .padding(8),
        ]
        .width(Fill)
        .height(Fill)
        .into()
    } else {
        column![toolbar, framebuffer_stack]
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
    vnc_action_button_inner(label, on_press, None, theme, fonts)
}

fn vnc_action_button_sized<'a>(
    label: &'a str,
    on_press: Message,
    width: f32,
    theme: Theme,
    fonts: ScaledFonts,
) -> Element<'a, Message> {
    vnc_action_button_inner(label, on_press, Some(width), theme, fonts)
}

fn vnc_action_button_inner<'a>(
    label: &'a str,
    on_press: Message,
    width: Option<f32>,
    theme: Theme,
    fonts: ScaledFonts,
) -> Element<'a, Message> {
    let label = container(text(label).size(fonts.small).color(theme.text_secondary))
        .width(width.map(Length::Fixed).unwrap_or(Length::Shrink))
        .align_x(iced::Alignment::Center);

    button(label)
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

fn vnc_metric_label<'a>(
    value: String,
    width: f32,
    color: iced::Color,
    theme: Theme,
    fonts: ScaledFonts,
) -> Element<'a, Message> {
    container(text(value).size(fonts.small).color(color))
        .width(Length::Fixed(width))
        .padding([2, 5])
        .style(move |_| iced::widget::container::Style {
            background: Some(theme.background.into()),
            border: iced::Border {
                radius: 3.0.into(),
                width: 1.0,
                color: theme.border,
            },
            ..Default::default()
        })
        .into()
}

fn format_update_age(stats: &VncStatsSnapshot) -> String {
    let Some(last_update_at) = stats.last_update_at else {
        return "pending".to_string();
    };

    let age_ms = last_update_at.elapsed().as_millis();
    if age_ms < 1_000 {
        format!("{:>3}ms", age_ms.min(999))
    } else {
        let age_s = age_ms / 1_000;
        if age_s > 99 {
            "99s+".to_string()
        } else {
            format!("{:>3}s", age_s)
        }
    }
}

fn format_fps(current_fps: f32, first_frame_received: bool) -> String {
    if !first_frame_received {
        return "-- fps".to_string();
    }

    let fps = if current_fps.is_finite() {
        current_fps.round().clamp(0.0, 20.0) as u32
    } else {
        0
    };
    format!("{:>2} fps", fps)
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

#[cfg(test)]
mod tests {
    use super::format_fps;

    #[test]
    fn format_fps_handles_pending_and_bad_values() {
        assert_eq!(format_fps(12.4, false), "-- fps");
        assert_eq!(format_fps(-5.0, true), " 0 fps");
        assert_eq!(format_fps(f32::NAN, true), " 0 fps");
        assert_eq!(format_fps(99.0, true), "20 fps");
    }
}
