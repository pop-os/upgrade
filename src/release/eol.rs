use anyhow::Context;
use chrono::{Date, NaiveDate, Utc};
use std::convert::TryFrom;
use ubuntu_version::{Codename, Version};

pub enum EolStatus {
    Ok,
    Imminent,
    Exceeded,
}

pub struct EolDate {
    pub version: Version,
    pub ymd:     (u32, u32, u32),
}

impl EolDate {
    pub fn fetch() -> anyhow::Result<Self> {
        let version = Version::detect().context("failed to detect current Ubuntu release")?;

        let codename = match Codename::try_from(version) {
            Ok(codename) => codename,
            Err(()) => return Err(anyhow!("Invalid Ubuntu version: {}", version)),
        };

        Ok(Self { version, ymd: codename.eol_date() })
    }

    pub fn status(&self) -> EolStatus {
        let (year, month, day) = self.ymd;
        let eol = ymd_to_utc(year as i32, month, day);
        let current = Utc::now().date();

        if current >= eol {
            EolStatus::Exceeded
        } else if imminent(current, eol) {
            EolStatus::Imminent
        } else {
            EolStatus::Ok
        }
    }
}

#[inline]
fn imminent(current: Date<Utc>, eol: Date<Utc>) -> bool {
    let days_until = eol.signed_duration_since(current).num_days();
    days_until >= 0 && days_until <= 30
}

fn ymd_to_utc(y: i32, m: u32, d: u32) -> Date<Utc> {
    Date::from_utc(NaiveDate::from_ymd(y, m, d), Utc)
}
