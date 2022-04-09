use std::path::Path;

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum SystemEnvironment {
    LegacyBios,
    Efi,
}

impl SystemEnvironment {
    pub fn detect() -> Self {
        if Path::new("/sys/firmware/efi").is_dir() {
            SystemEnvironment::Efi
        } else {
            SystemEnvironment::LegacyBios
        }
    }
}
