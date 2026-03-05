// SPDX-License-Identifier: GPL-3.0-only

mod app;
mod config;
mod dbus;
mod i18n;
mod pages;

fn main() -> cosmic::iced::Result {
    let requested_languages = i18n_embed::DesktopLanguageRequester::requested_languages();
    i18n::init(&requested_languages);

    let settings = cosmic::app::Settings::default()
        .size_limits(cosmic::iced::Limits::NONE.min_width(480.0).min_height(360.0));

    cosmic::app::run::<app::AppModel>(settings, ())
}
