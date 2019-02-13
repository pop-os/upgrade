use fern::{Dispatch, InitError};
use log::{Level, LevelFilter};
use std::io;
use yansi::Color;

pub fn setup_logging(filter: LevelFilter) -> Result<(), InitError> {
    Dispatch::new()
        // Exclude logs for crates that we use
        .level(LevelFilter::Off)
        // Include only the logs for this binary
        .level_for("pop_upgrade", filter)
        .level_for("apt_fetcher", filter)
        .format(|out, message, record| {
            let color = match record.level() {
                Level::Trace => Color::Cyan.style().bold(),
                Level::Debug => Color::Red.style().bold(),
                Level::Error => Color::Red.style().bold(),
                Level::Warn => Color::Yellow.style().bold(),
                Level::Info => Color::Green.style().bold(),
            };

            out.finish(format_args!(" {} {}", color.paint(record.level()), message))
        })
        .chain(io::stderr())
        .apply()?;
    Ok(())
}
