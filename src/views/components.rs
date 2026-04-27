//! Shared lightweight UI primitives used across Portal views.

use iced::widget::{Column, Row, Space, button, column, container, row, text, tooltip};
use iced::{Alignment, Element, Fill, Length, Padding};

use crate::theme::{
    BORDER_RADIUS, CARD_BORDER_RADIUS, STATUS_FAILURE, STATUS_PARTIAL, STATUS_SUCCESS, ScaledFonts,
    Theme,
};

#[derive(Debug, Clone, Copy)]
pub enum BadgeTone {
    Neutral,
    Info,
    Success,
    Warning,
    Danger,
}

pub fn field<'a, Message: 'a>(
    label: impl Into<String>,
    description: impl Into<String>,
    control: impl Into<Element<'a, Message>>,
    theme: Theme,
    fonts: ScaledFonts,
) -> Element<'a, Message> {
    row![
        iced::widget::column![
            text(label.into())
                .size(fonts.body)
                .color(theme.text_primary),
            Space::new().height(4),
            text(description.into())
                .size(fonts.label)
                .color(theme.text_muted),
        ]
        .spacing(0),
        Space::new().width(Length::Fill),
        control.into(),
    ]
    .align_y(Alignment::Center)
    .into()
}

pub fn form_card<'a, Message: 'a>(
    items: Vec<Element<'a, Message>>,
    theme: Theme,
) -> Element<'a, Message> {
    let mut content = Column::new().spacing(16).padding(20);
    for item in items {
        content = content.push(item);
    }

    container(content)
        .width(Length::Fill)
        .style(move |_| container::Style {
            background: Some(theme.surface.into()),
            border: iced::Border {
                radius: CARD_BORDER_RADIUS.into(),
                ..Default::default()
            },
            ..Default::default()
        })
        .into()
}

pub fn status_badge<'a, Message: 'a>(
    label: impl Into<String>,
    tone: BadgeTone,
    theme: Theme,
    fonts: ScaledFonts,
) -> Element<'a, Message> {
    let (foreground, background) = match tone {
        BadgeTone::Neutral => (theme.text_secondary, theme.background),
        BadgeTone::Info => (theme.accent, translucent(theme.accent, 0.14)),
        BadgeTone::Success => (STATUS_SUCCESS, translucent(STATUS_SUCCESS, 0.14)),
        BadgeTone::Warning => (STATUS_PARTIAL, translucent(STATUS_PARTIAL, 0.16)),
        BadgeTone::Danger => (STATUS_FAILURE, translucent(STATUS_FAILURE, 0.14)),
    };

    container(
        text(label.into())
            .size(fonts.small)
            .color(foreground)
            .width(Length::Shrink),
    )
    .padding([3, 8])
    .style(move |_| container::Style {
        background: Some(background.into()),
        border: iced::Border {
            color: translucent(foreground, 0.34),
            width: 1.0,
            radius: 999.0.into(),
        },
        ..Default::default()
    })
    .into()
}

pub fn kbd<'a, Message: 'a>(
    label: impl Into<String>,
    theme: Theme,
    fonts: ScaledFonts,
) -> Element<'a, Message> {
    container(
        text(label.into())
            .size(fonts.small)
            .color(theme.text_secondary),
    )
    .padding([2, 7])
    .style(move |_| container::Style {
        background: Some(theme.background.into()),
        border: iced::Border {
            color: theme.border,
            width: 1.0,
            radius: 4.0.into(),
        },
        shadow: iced::Shadow {
            color: iced::Color {
                a: 0.18,
                ..iced::Color::BLACK
            },
            offset: iced::Vector::new(0.0, 1.0),
            blur_radius: 0.0,
        },
        ..Default::default()
    })
    .into()
}

pub fn help_tooltip<'a, Message: 'a>(
    trigger: impl Into<Element<'a, Message>>,
    label: impl Into<String>,
    theme: Theme,
    fonts: ScaledFonts,
    position: tooltip::Position,
) -> Element<'a, Message> {
    tooltip(
        trigger,
        text(label.into())
            .size(fonts.label)
            .color(theme.text_secondary),
        position,
    )
    .style(move |_| container::Style {
        background: Some(theme.surface.into()),
        border: iced::Border {
            color: theme.border,
            width: 1.0,
            radius: 4.0.into(),
        },
        ..Default::default()
    })
    .padding(8)
    .gap(6)
    .into()
}

pub fn progress_bar<'a, Message: 'a>(
    progress: f32,
    theme: Theme,
    height: f32,
) -> Element<'a, Message> {
    let progress = progress.clamp(0.0, 1.0);
    let fill = container(Space::new().width(Fill).height(Length::Fixed(height))).style(move |_| {
        container::Style {
            background: Some(theme.accent.into()),
            border: iced::Border {
                radius: (height / 2.0).into(),
                ..Default::default()
            },
            ..Default::default()
        }
    });

    container(
        row![
            container(fill).width(Length::FillPortion((progress * 1000.0).round() as u16)),
            Space::new().width(Length::FillPortion(
                ((1.0 - progress) * 1000.0).round() as u16
            )),
        ]
        .height(Length::Fixed(height)),
    )
    .width(Fill)
    .height(Length::Fixed(height))
    .style(move |_| container::Style {
        background: Some(theme.background.into()),
        border: iced::Border {
            color: theme.border,
            width: 1.0,
            radius: (height / 2.0).into(),
        },
        ..Default::default()
    })
    .into()
}

pub fn skeleton_rows<'a, Message: 'a>(
    rows: usize,
    theme: Theme,
    fonts: ScaledFonts,
) -> Element<'a, Message> {
    let mut content = Column::new().spacing(10).padding(Padding::from([16, 12]));

    for index in 0..rows {
        let width = match index % 3 {
            0 => 86,
            1 => 68,
            _ => 76,
        };
        let row = row![
            skeleton_block(24.0, 24.0, 6.0, theme),
            skeleton_block(width as f32, fonts.body + 5.0, 4.0, theme),
            Space::new().width(Fill),
            skeleton_block(92.0, fonts.label + 5.0, 4.0, theme),
            skeleton_block(52.0, fonts.label + 5.0, 4.0, theme),
        ]
        .spacing(12)
        .align_y(Alignment::Center);
        content = content.push(row);
    }

    column![
        progress_bar(0.42, theme, 3.0),
        container(content).width(Fill)
    ]
    .spacing(0)
    .into()
}

pub fn dropzone_overlay<'a, Message: 'a>(
    label: impl Into<String>,
    theme: Theme,
    fonts: ScaledFonts,
) -> Element<'a, Message> {
    container(
        container(
            column![
                text(label.into())
                    .size(fonts.section)
                    .color(theme.text_primary),
                text("Release to copy into the active pane")
                    .size(fonts.label)
                    .color(theme.text_secondary),
            ]
            .spacing(6)
            .align_x(Alignment::Center),
        )
        .padding(28)
        .style(move |_| container::Style {
            background: Some(theme.surface.into()),
            border: iced::Border {
                color: theme.accent,
                width: 2.0,
                radius: CARD_BORDER_RADIUS.into(),
            },
            shadow: iced::Shadow {
                color: iced::Color {
                    a: 0.35,
                    ..iced::Color::BLACK
                },
                offset: iced::Vector::new(0.0, 4.0),
                blur_radius: 18.0,
            },
            ..Default::default()
        }),
    )
    .width(Fill)
    .height(Fill)
    .align_x(Alignment::Center)
    .align_y(Alignment::Center)
    .style(move |_| container::Style {
        background: Some(iced::Color::from_rgba(0.0, 0.0, 0.0, 0.42).into()),
        ..Default::default()
    })
    .into()
}

pub fn toggle_group<'a, T, Message, F>(
    current: T,
    options: &[(T, &'static str)],
    on_select: F,
    theme: Theme,
    fonts: ScaledFonts,
) -> Element<'a, Message>
where
    T: Copy + PartialEq + 'a,
    Message: Clone + 'a,
    F: Fn(T) -> Message + Copy + 'a,
{
    let mut controls = Row::new().spacing(2).align_y(Alignment::Center);

    for &(value, label) in options {
        controls = controls.push(toggle_group_button(
            label,
            value == current,
            on_select(value),
            theme,
            fonts,
        ));
    }

    container(controls)
        .padding(3)
        .style(move |_| container::Style {
            background: Some(theme.background.into()),
            border: iced::Border {
                color: theme.border,
                width: 1.0,
                radius: BORDER_RADIUS.into(),
            },
            ..Default::default()
        })
        .into()
}

fn toggle_group_button<'a, Message: Clone + 'a>(
    label: &'static str,
    selected: bool,
    on_press: Message,
    theme: Theme,
    fonts: ScaledFonts,
) -> Element<'a, Message> {
    let text_color = if selected {
        theme.text_primary
    } else {
        theme.text_secondary
    };

    button(
        container(text(label).size(fonts.label).color(text_color))
            .padding([6, 10])
            .align_x(Alignment::Center)
            .align_y(Alignment::Center),
    )
    .padding(0)
    .style(move |_theme, status| {
        let background = match (selected, status) {
            (true, _) => Some(theme.selected.into()),
            (false, button::Status::Hovered) => Some(theme.hover.into()),
            (false, _) => None,
        };

        button::Style {
            background,
            text_color,
            border: iced::Border {
                color: if selected {
                    theme.accent
                } else {
                    iced::Color::TRANSPARENT
                },
                width: if selected { 1.0 } else { 0.0 },
                radius: (BORDER_RADIUS - 2.0).into(),
            },
            ..Default::default()
        }
    })
    .on_press(on_press)
    .into()
}

fn translucent(color: iced::Color, alpha: f32) -> iced::Color {
    iced::Color { a: alpha, ..color }
}

fn skeleton_block<'a, Message: 'a>(
    width: f32,
    height: f32,
    radius: f32,
    theme: Theme,
) -> Element<'a, Message> {
    container(Space::new().width(width).height(height))
        .style(move |_| container::Style {
            background: Some(theme.hover.into()),
            border: iced::Border {
                radius: radius.into(),
                ..Default::default()
            },
            ..Default::default()
        })
        .into()
}
