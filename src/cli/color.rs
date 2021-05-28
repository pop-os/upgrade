use std::fmt::Display;
use yansi::Paint;

pub(crate) fn error<T: Display>(value: T) -> Paint<T> { Paint::red(value).bold() }

pub(crate) fn error_desc<T: Display>(value: T) -> Paint<T> { Paint::red(value).bold().dimmed() }

pub(crate) fn info<T: Display>(value: T) -> Paint<T> { Paint::green(value).bold() }

pub(crate) fn primary<T: Display>(value: T) -> Paint<T> { Paint::cyan(value).bold() }

pub(crate) fn secondary<T: Display>(value: T) -> Paint<T> { Paint::blue(value).bold() }
