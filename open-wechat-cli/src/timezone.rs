use crate::cli::Cli;
use crate::config::Config;
use crate::output::{OwcError, Result};
use chrono::{Local, NaiveDate, NaiveDateTime, TimeZone};
use chrono_tz::Tz;

#[derive(Debug, Clone, Copy)]
pub enum Timezone {
    Local,
    Named(Tz),
}

pub fn resolve(cli: &Cli, config: &Config) -> Result<Timezone> {
    let name = cli.tz.as_deref().unwrap_or(config.tz.as_str());
    let name = if name.is_empty() { "local" } else { name };
    if name == "local" {
        Ok(Timezone::Local)
    } else {
        name.parse::<Tz>()
            .map(Timezone::Named)
            .map_err(|_| OwcError::usage(format!("Unknown timezone `{name}`.")))
    }
}

pub fn parse_bound(
    value: Option<&str>,
    timezone: Timezone,
    end_of_day: bool,
) -> Result<Option<i64>> {
    let Some(value) = value else {
        return Ok(None);
    };
    let date_only = value.len() == 10;
    let naive = if date_only {
        NaiveDate::parse_from_str(value, "%Y-%m-%d")
            .map_err(|_| {
                OwcError::usage(format!(
                    "Invalid time `{value}`; use YYYY-MM-DD or YYYY-MM-DDTHH:MM:SS."
                ))
            })?
            .and_hms_opt(
                if end_of_day { 23 } else { 0 },
                if end_of_day { 59 } else { 0 },
                if end_of_day { 59 } else { 0 },
            )
            .ok_or_else(|| OwcError::usage(format!("Invalid time `{value}`.")))?
    } else {
        NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M:%S")
            .or_else(|_| NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S"))
            .map_err(|_| {
                OwcError::usage(format!(
                    "Invalid time `{value}`; use YYYY-MM-DD or YYYY-MM-DDTHH:MM:SS."
                ))
            })?
    };
    let timestamp = match timezone {
        Timezone::Local => Local
            .from_local_datetime(&naive)
            .single()
            .or_else(|| Local.from_local_datetime(&naive).earliest())
            .ok_or_else(|| {
                OwcError::usage(format!("Time `{value}` is invalid in the local timezone."))
            })?
            .timestamp(),
        Timezone::Named(tz) => tz
            .from_local_datetime(&naive)
            .single()
            .or_else(|| tz.from_local_datetime(&naive).earliest())
            .ok_or_else(|| {
                OwcError::usage(format!("Time `{value}` is invalid in timezone `{tz}`."))
            })?
            .timestamp(),
    };
    Ok(Some(timestamp))
}

pub fn format_epoch(epoch: i64, timezone: Timezone) -> String {
    chrono::DateTime::from_timestamp(epoch, 0)
        .map(|date| match timezone {
            Timezone::Local => date
                .with_timezone(&Local)
                .format("%Y-%m-%dT%H:%M:%S")
                .to_string(),
            Timezone::Named(tz) => date
                .with_timezone(&tz)
                .format("%Y-%m-%dT%H:%M:%S")
                .to_string(),
        })
        .unwrap_or_default()
}
