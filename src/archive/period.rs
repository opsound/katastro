//! Calendar periods for statistics on the archive's displayed play dates.
use super::Server;
use serde::{Deserialize, Serialize};
use std::{fmt, ops::RangeInclusive};
use time::{
    Date, Duration, OffsetDateTime, Time, UtcOffset,
    macros::{format_description, offset, time},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Period {
    Last30,
    #[default]
    Last90,
    ThisYear,
    AllTime,
    Custom(Interval),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "RangeInclusive<Date>")]
pub struct Interval {
    pub start: Date,
    pub end: Date,
}

impl Interval {
    pub fn new(start: Date, end: Date) -> Option<Self> {
        (start <= end).then_some(Self { start, end })
    }

    fn preceding(self) -> Option<Self> {
        Some(Self {
            start: self
                .start
                .checked_sub(self.end - self.start + Duration::DAY)?,
            end: self.start.previous_day()?,
        })
    }

    fn contains(self, date: Date) -> bool {
        (self.start..=self.end).contains(&date)
    }
}

impl TryFrom<RangeInclusive<Date>> for Interval {
    type Error = &'static str;

    fn try_from(range: RangeInclusive<Date>) -> Result<Self, Self::Error> {
        let (start, end) = range.into_inner();
        Self::new(start, end).ok_or("End date must be on or after the start date")
    }
}

impl fmt::Display for Interval {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} – {}", self.start, self.end)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bucket {
    Current,
    Previous,
}

#[derive(Clone, Copy, Debug)]
pub struct Windows {
    pub current: Option<Interval>,
    pub previous: Option<Interval>,
}

impl Period {
    pub fn windows(self, today: Date) -> Windows {
        let through_today = |start| Interval { start, end: today };
        let current = match self {
            Self::Last30 => Some(through_today(today.saturating_sub(Duration::days(29)))),
            Self::Last90 => Some(through_today(today.saturating_sub(Duration::days(89)))),
            Self::ThisYear => Some(through_today(
                Date::from_ordinal_date(today.year(), 1).unwrap(),
            )),
            Self::Custom(interval) => Some(interval),
            Self::AllTime => None,
        };
        Windows {
            current,
            previous: current.and_then(Interval::preceding),
        }
    }
}

impl Windows {
    pub fn bucket(self, archive_date: &str) -> Option<Bucket> {
        let Some(current) = self.current else {
            return Some(Bucket::Current);
        };
        let text = archive_date.trim().split([' ', 'T']).next()?;
        let date = parse_date(text)?;
        if current.contains(date) {
            Some(Bucket::Current)
        } else {
            self.previous
                .filter(|range| range.contains(date))
                .map(|_| Bucket::Previous)
        }
    }
}

pub(super) fn parse_date(text: &str) -> Option<Date> {
    let format = if text.as_bytes().get(4) == Some(&b'/') {
        format_description!("[year]/[month]/[day]")
    } else {
        format_description!("[year]-[month]-[day]")
    };
    Date::parse(text, format).ok()
}

pub(super) fn play_date_time(text: &str) -> Option<(Date, Option<Time>)> {
    let text = text.trim();
    let (day, clock) = text.split_once([' ', 'T']).unwrap_or((text, ""));
    let day = parse_date(day)?;
    let clock = Time::parse(
        clock,
        format_description!("[hour]:[minute][optional [:[second]]]"),
    )
    .ok();
    Some((day, clock))
}

/// The index retains server time. Convert timed records when loading a library,
/// using the Mac's offset on the day played. Date-only records have no instant.
pub(super) fn local_play_time(server: Server, recorded: String) -> (String, Option<i64>) {
    let Some((day, Some(clock))) = play_date_time(&recorded) else {
        // Place date-only observations at local noon while displaying just the recorded day.
        let timestamp = parse_date(recorded.trim()).map(|date| {
            let noon = date.with_time(time!(12:00));
            noon.assume_offset(local_offset(noon.assume_utc()))
                .unix_timestamp()
        });
        return (recorded, timestamp);
    };
    let instant = day.with_time(clock).assume_offset(match server {
        Server::Pandanet => offset!(+9),
        Server::Fox => offset!(+8),
        Server::Ogs => UtcOffset::UTC,
    });
    let format = if recorded.matches(':').count() == 2 {
        format_description!("[year]/[month]/[day] [hour]:[minute]:[second]")
    } else {
        format_description!("[year]/[month]/[day] [hour]:[minute]")
    };
    let local = instant
        .checked_to_offset(local_offset(instant))
        .and_then(|local| local.format(format).ok())
        .unwrap_or(recorded);
    (local, Some(instant.unix_timestamp()))
}

pub(super) fn local_offset(instant: OffsetDateTime) -> UtcOffset {
    UtcOffset::local_offset_at(instant).unwrap_or(UtcOffset::UTC)
}

pub fn local_today() -> Date {
    OffsetDateTime::now_local()
        .unwrap_or_else(|_| OffsetDateTime::now_utc())
        .date()
}
