use yansi::Paint;

pub(crate) fn color_error<T: ::std::fmt::Display>(value: T) -> Paint<T> {
    Paint::red(value).bold()
}

pub(crate) fn color_error_desc<T: ::std::fmt::Display>(value: T) -> Paint<T> {
    Paint::red(value).bold().dimmed()
}

pub(crate) fn color_info<T: ::std::fmt::Display>(value: T) -> Paint<T> {
    Paint::yellow(value).bold()
}

pub(crate) fn color_primary<T: ::std::fmt::Display>(value: T) -> Paint<T> {
    Paint::green(value).bold()
}

pub(crate) fn color_secondary<T: ::std::fmt::Display>(value: T) -> Paint<T> {
    Paint::cyan(value).bold()
}

pub(crate) fn color_tertiary<T: ::std::fmt::Display>(value: T) -> Paint<T> {
    Paint::blue(value).bold()
}
