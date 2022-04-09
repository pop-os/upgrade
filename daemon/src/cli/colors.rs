use std::fmt::Display;
use yansi::Paint;

pub(crate) fn color_error<T: Display>(value: T) -> Paint<T> { Paint::red(value).bold() }

pub(crate) fn color_error_desc<T: Display>(value: T) -> Paint<T> {
    Paint::red(value).bold().dimmed()
}

pub(crate) fn color_info<T: Display>(value: T) -> Paint<T> { Paint::green(value).bold() }

pub(crate) fn color_primary<T: Display>(value: T) -> Paint<T> { Paint::cyan(value).bold() }

pub(crate) fn color_secondary<T: Display>(value: T) -> Paint<T> { Paint::blue(value).bold() }
