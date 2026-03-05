// SPDX-License-Identifier: GPL-3.0-only

//! Fans page: per-channel monitoring, override controls, and active curves.

use crate::app::{AppModel, FanChannel, Message};
use crate::fl;
use cosmic::iced::Length;
use cosmic::prelude::*;
use cosmic::widget;

/// Format a channel's display name, preferring the sysfs label when available.
///
/// Returns "CPU Fan (pwm1)" if a label exists, otherwise just "pwm1".
fn display_name(ch: &FanChannel) -> String {
    match &ch.label {
        Some(label) => format!("{label} ({name})", name = ch.name),
        None => ch.name.clone(),
    }
}

/// Build the fans page view.
pub fn view(app: &AppModel) -> Element<'_, Message> {
    let space_s = cosmic::theme::spacing().space_s;
    let space_m = cosmic::theme::spacing().space_m;

    // Fan channel status table.
    let channel_section = {
        let mut col = widget::column().spacing(space_s);

        if app.fan_channels.is_empty() && app.connected {
            col = col.push(widget::text::body(fl!("no-fan-channels")));
        }

        for ch in &app.fan_channels {
            let dname = display_name(ch);
            if ch.passthrough {
                col = col.push(
                    cosmic::widget::settings::item::builder(dname)
                        .control(widget::text::body(fl!("passthrough"))),
                );
                continue;
            }

            let duty_str = if ch.duty_raw >= 0 {
                let pct = (ch.duty_raw as f64 / 255.0) * 100.0;
                format!("{pct:.0}%")
            } else {
                "--".into()
            };

            let rpm_str = if ch.rpm >= 0 { format!("{} RPM", ch.rpm) } else { String::new() };

            let mut detail = duty_str;
            if !rpm_str.is_empty() {
                detail.push_str(&format!("  {rpm_str}"));
            }
            if ch.stalled {
                detail.push_str(&format!("  {}", fl!("stalled")));
            }
            if let Some(pct) = ch.override_pct {
                detail.push_str(&format!("  [override {pct}%]"));
            }

            col = col.push(
                cosmic::widget::settings::item::builder(dname).control(widget::text::body(detail)),
            );
        }

        cosmic::widget::settings::section().title(fl!("fan-channels")).add(col)
    };

    // Fan override controls.
    let override_section = build_override_section(app, space_s);

    // Active fan curves.
    let mut layout = widget::column().push(channel_section).push(override_section).spacing(space_m);

    if !app.fan_curves.is_empty() {
        let mut col = widget::column().spacing(space_s);

        for (name, points) in &app.fan_curves {
            let pts: String = points
                .iter()
                .map(|(t, d)| format!("{t:.0}C/{d:.0}%"))
                .collect::<Vec<_>>()
                .join("  ");
            col = col.push(
                cosmic::widget::settings::item::builder(name).control(widget::text::body(pts)),
            );
        }

        let curve_section = cosmic::widget::settings::section().title(fl!("fan-curves")).add(col);

        layout = layout.push(curve_section);
    }

    layout.width(Length::Fill).height(Length::Fill).into()
}

/// Build the fan override controls section.
///
/// Separated to keep the main view function readable. Each non-passthrough
/// channel gets a text input for the duty percentage and set/clear buttons.
fn build_override_section<'a>(
    app: &'a AppModel,
    spacing: u16,
) -> cosmic::widget::settings::Section<'a, Message> {
    let mut col = widget::column().spacing(spacing);
    let has_controllable = app.fan_channels.iter().any(|ch| !ch.passthrough);

    if !has_controllable {
        col = col.push(widget::text::body(fl!("no-controllable-fans")));
    }

    for ch in &app.fan_channels {
        if ch.passthrough {
            continue;
        }

        let input_val: &str = app.override_inputs.get(&ch.name).map(String::as_str).unwrap_or("");

        let parsed = input_val.parse::<u8>().ok().filter(|&d| d <= 100);

        let name_input = ch.name.clone();
        let name_set = ch.name.clone();
        let name_clear = ch.name.clone();

        let placeholder = fl!("duty-placeholder");

        let dname = display_name(ch);
        let row =
            widget::row()
                .push(widget::text::body(dname).width(160))
                .push(
                    widget::text_input(placeholder, input_val)
                        .on_input(move |v| Message::OverrideInputChanged {
                            channel: name_input.clone(),
                            value: v,
                        })
                        .width(80),
                )
                .push(widget::text::body("%"))
                .push(widget::button::text(fl!("set")).on_press_maybe(parsed.map(|d| {
                    Message::SetFanOverride { channel: name_set.clone(), duty_percent: d }
                })))
                .push(
                    widget::button::text(fl!("clear"))
                        .on_press(Message::ClearFanOverride(name_clear)),
                )
                .spacing(spacing)
                .align_y(cosmic::iced::Alignment::Center);

        col = col.push(row);
    }

    cosmic::widget::settings::section().title(fl!("fan-overrides")).add(col)
}
