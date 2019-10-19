#[macro_use]
extern crate cascade;

use gio::{prelude::*, ApplicationFlags};
use gtk::{prelude::*, Application};
use pop_upgrade_gtk::*;

pub const APP_ID: &str = "com.system76.UpgradeManager";

fn main() {
    glib::set_program_name(APP_ID.into());

    let application = Application::new(Some(APP_ID), ApplicationFlags::empty())
        .expect("GTK initialization failed");

    application.connect_activate(|app| {
        if let Some(window) = app.get_window_by_id(0) {
            window.present();
        }
    });

    application.connect_startup(|app| {
        let widget = UpgradeWidget::new();
        widget.scan();

        let headerbar = cascade! {
            gtk::HeaderBar::new();
            ..set_title(Some("Pop! Upgrade Manager"));
            ..set_show_close_button(true);
            ..show();
        };

        let _window = cascade! {
            gtk::ApplicationWindow::new(app);
            ..set_titlebar(Some(&headerbar));
            ..set_icon_name(Some("firmware-manager"));
            ..set_keep_above(true);
            ..set_property_window_position(gtk::WindowPosition::Center);
            ..add(cascade! {
                widget.as_ref();
                ..set_border_width(12);
                ..set_margin_top(24);
                ..set_halign(gtk::Align::Center);
            });
            ..show();
        };

        app.connect_shutdown(move |_| widget.shutdown());
    });

    application.run(&[]);
}

/// Manages argument parsing for the GTK application via clap.
///
/// Currently the primary purpose is to determine the logging level.
fn argument_parsing() {
    use clap::{App, Arg};
    use log::LevelFilter;

    let matches = App::new("com.system76.FirmwareManager")
        .arg(
            Arg::with_name("verbose")
                .short("v")
                .multiple(true)
                .help("define the logging level; multiple occurrences increases the logging level"),
        )
        .get_matches();

    let logging_level = match matches.occurrences_of("verbose") {
        0 => LevelFilter::Info,
        1 => LevelFilter::Debug,
        _ => LevelFilter::Trace,
    };

    if let Err(why) = install_logging(logging_level) {
        eprintln!("failed to initiate logging: {}", why);
    }
}

use fern::{Dispatch, InitError};
use log::{Level, LevelFilter, Record};
use std::io;
use yansi::Paint;

fn install_logging(filter: LevelFilter) -> Result<(), InitError> {
    let location = |record: &Record| {
        let mut target = record.target();
        if let Some(pos) = target.find(':') {
            target = &target[..pos];
        }

        match (record.file(), record.line()) {
            (Some(file), Some(line)) => format!(
                "{} {}{}{}",
                Paint::cyan(target).bold(),
                Paint::blue(file).bold(),
                Paint::new(":").bold(),
                Paint::magenta(line).bold()
            ),
            _ => String::new(),
        }
    };

    let format_level = |record: &Record| match record.level() {
        level @ Level::Trace => Paint::green(level).bold(),
        level @ Level::Warn => Paint::yellow(level).bold(),
        level @ Level::Error => Paint::red(level).bold(),
        level => Paint::new(level).bold(),
    };

    Dispatch::new()
        // Exclude logs for crates that we use
        .level(LevelFilter::Off)
        // Include only the logs for relevant crates of interest
        .level_for("pop_upgrade", filter)
        .level_for("pop_upgrade_gtk", filter)
        .level_for("apt_fetcher", filter)
        .format(move |out, message, record| {
            out.finish(format_args!(
                "[{:5}] {}: {}",
                format_level(record),
                location(record),
                message
            ))
        })
        .chain(io::stderr())
        .apply()?;
    Ok(())
}
