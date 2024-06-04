use yansi::Painted;

pub(crate) fn color_error<T>(value: T) -> Painted<T> { Painted::new(value).red().bold() }

pub(crate) fn color_error_desc<T>(value: T) -> Painted<T> { Painted::new(value).red().bold().dim() }

pub(crate) fn color_info<T>(value: T) -> Painted<T> { Painted::new(value).green().bold() }

pub(crate) fn color_primary<T>(value: T) -> Painted<T> { Painted::new(value).cyan().bold() }

pub(crate) fn color_secondary<T>(value: T) -> Painted<T> { Painted::new(value).blue().bold() }
