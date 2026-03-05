// SPDX-License-Identifier: GPL-3.0-only

//! Overview page: power profile switcher, temperature readouts, and status.

use crate::app::{AppModel, Message};
use crate::fl;
use cosmic::iced::Length;
use cosmic::prelude::*;
use cosmic::widget;

/// Build the overview page view.
pub fn view(app: &AppModel) -> Element<'_, Message> {
    let space_s = cosmic::theme::spacing().space_s;
    let space_m = cosmic::theme::spacing().space_m;

    // Profile selector: three radio buttons.
    let profile_section = {
        let quiet = widget::radio(
            widget::text::body(fl!("quiet")),
            "Quiet",
            Some(app.profile.as_str()),
            |_| Message::SetProfile("Quiet".into()),
        );
        let balanced = widget::radio(
            widget::text::body(fl!("balanced")),
            "Balanced",
            Some(app.profile.as_str()),
            |_| Message::SetProfile("Balanced".into()),
        );
        let performance = widget::radio(
            widget::text::body(fl!("performance")),
            "Performance",
            Some(app.profile.as_str()),
            |_| Message::SetProfile("Performance".into()),
        );

        let radios = widget::column().push(quiet).push(balanced).push(performance).spacing(space_s);

        cosmic::widget::settings::section().title(fl!("power-profile")).add(radios)
    };

    // Temperature readouts.
    let temp_section = {
        let mut col = widget::column().spacing(space_s);

        if let Some(cpu) = app.cpu_temp {
            col = col.push(
                cosmic::widget::settings::item::builder(fl!("cpu-temp"))
                    .control(widget::text::body(format!("{cpu:.1} C"))),
            );
        }
        if let Some(gpu) = app.gpu_temp {
            col = col.push(
                cosmic::widget::settings::item::builder(fl!("gpu-temp"))
                    .control(widget::text::body(format!("{gpu:.1} C"))),
            );
        }

        if app.cpu_temp.is_none() && app.gpu_temp.is_none() {
            col = col.push(widget::text::body(fl!("no-temps")));
        }

        cosmic::widget::settings::section().title(fl!("temperatures")).add(col)
    };

    // Status indicators. Track whether we have anything to show.
    let has_status =
        !app.connected || !app.config_loaded || app.critical || app.error_message.is_some();

    let mut layout = widget::column().push(profile_section).push(temp_section).spacing(space_m);

    if has_status {
        let mut col = widget::column().spacing(space_s);

        if !app.connected {
            col = col.push(widget::text::body(fl!("daemon-offline")));
        }
        if !app.config_loaded && app.connected {
            col = col.push(widget::text::body(fl!("no-fan-config")));
        }
        if app.critical {
            col = col.push(widget::text::body(fl!("critical-temp")));
        }
        if let Some(ref err) = app.error_message {
            col = col.push(widget::text::body(err.clone()));
        }

        let status_section = cosmic::widget::settings::section().title(fl!("status")).add(col);

        layout = layout.push(status_section);
    }

    layout.width(Length::Fill).height(Length::Fill).into()
}
