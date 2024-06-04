use crate::ubuntu_version::{Codename, Version};
use anyhow::Context;
use chrono::{Date, NaiveDate, Utc};
use std::convert::TryFrom;

#[derive(Debug, PartialEq)]
pub enum EolStatus {
    Ok,
    Imminent,
    Exceeded,
}

pub struct EolDate {
    pub codename: Codename,
    pub version:  Version,
    pub ymd:      (u32, u32, u32),
}

impl From<Codename> for EolDate {
    fn from(codename: Codename) -> Self {
        Self { codename, version: codename.into(), ymd: codename.eol_date() }
    }
}

impl EolDate {
    pub fn fetch() -> anyhow::Result<Self> {
        let version = Version::detect().context("failed to detect current Ubuntu release")?;

        let codename = match Codename::try_from(version) {
            Ok(codename) => codename,
            Err(()) => return Err(anyhow!("Invalid Ubuntu version: {}", version)),
        };

        Ok(Self { codename, version, ymd: codename.eol_date() })
    }

    #[inline]
    pub fn status(&self) -> EolStatus { self.status_from(Utc::now().date()) }

    pub fn status_from(&self, date: Date<Utc>) -> EolStatus {
        let (year, month, day) = self.ymd;
        let eol = ymd_to_utc(year as i32, month, day);

        if date >= eol {
            EolStatus::Exceeded
        } else if imminent(date, eol, self.codename) {
            EolStatus::Imminent
        } else {
            EolStatus::Ok
        }
    }
}

#[inline]
fn imminent(current: Date<Utc>, eol: Date<Utc>, codename: Codename) -> bool {
    let days_until = eol.signed_duration_since(current).num_days();
    let days_left = if codename == Codename::Groovy { 7 } else { 30 };
    days_until >= 0 && days_until <= days_left
}

fn ymd_to_utc(y: i32, m: u32, d: u32) -> Date<Utc> {
    Date::from_utc(NaiveDate::from_ymd(y, m, d), Utc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ubuntu_version::Codename;

    #[test]
    fn eol_exceeded() {
        let disco = EolDate::from(Codename::Disco);
        assert_eq!(disco.status_from(ymd_to_utc(2020, 1, 18)), EolStatus::Exceeded);
        assert_eq!(disco.status_from(ymd_to_utc(2020, 2, 1)), EolStatus::Exceeded);
        assert_eq!(disco.status_from(ymd_to_utc(2021, 1, 1)), EolStatus::Exceeded);
    }

    #[test]
    fn eol_imminent() {
        let disco = EolDate::from(Codename::Disco);
        assert_eq!(disco.status_from(ymd_to_utc(2019, 12, 30)), EolStatus::Imminent);
        assert_eq!(disco.status_from(ymd_to_utc(2020, 1, 17)), EolStatus::Imminent);
    }

    #[test]
    fn eol_ok() {
        let disco = EolDate::from(Codename::Disco);
        assert_eq!(disco.status_from(ymd_to_utc(2019, 10, 30)), EolStatus::Ok);
        assert_eq!(disco.status_from(ymd_to_utc(2019, 12, 10)), EolStatus::Ok);
    }
}
