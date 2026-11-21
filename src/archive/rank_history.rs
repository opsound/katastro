//! The synced player's recorded rank over the dates in one server archive.
use super::{Server, Snapshot};
use time::{Date, Time, UtcDateTime, macros::format_description};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RankPoint {
    /// Seconds since the Unix epoch using the wall clock displayed by the archive.
    pub timestamp: i64,
    /// Half-rank steps. 1k is zero, 1d is two and the professional band starts at twenty.
    pub level: i32,
}

fn parts(value: &str) -> Option<(i32, u8)> {
    let value = value.trim_end_matches(['+', '-', '*', '?']);
    let (&unit, digits) = value.as_bytes().split_last()?;
    if !digits.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let number = str::from_utf8(digits).ok()?.parse::<i32>().ok()?;
    let unit = unit.to_ascii_lowercase();
    ((1..100).contains(&number) && matches!(unit, b'k' | b'd' | b'p')).then_some((number, unit))
}

/// Split the indexed player/rank field while retaining spaces in account names.
pub(super) fn player(value: &str) -> (&str, &str) {
    let value = value.trim();
    if let Some((name, rank)) = value.rsplit_once(char::is_whitespace)
        && (parts(rank).is_some() || matches!(rank, "NR" | "?" | "-"))
    {
        return (name.trim_end(), rank);
    }
    (value, "—")
}

fn level(rank: &str, server: Server) -> Option<i32> {
    let (number, unit) = parts(rank)?;
    let whole = match unit {
        b'k' => 1 - number,
        b'd' if number <= 9 => number,
        b'p' if number <= 9 => 9 + number,
        _ => return None,
    };
    let upper_half = server == Server::Pandanet && unit != b'p' && rank.contains('+');
    Some(2 * whole + i32::from(upper_half))
}

pub fn rank_label(level: i32) -> String {
    let whole = level.div_euclid(2);
    let half = if level.rem_euclid(2) == 0 { "" } else { "+" };
    match whole {
        -98..=0 => format!("{}k{half}", 1 - whole),
        1..=9 => format!("{whole}d{half}"),
        10..=18 if half.is_empty() => format!("{}p", whole - 9),
        _ => String::new(),
    }
}

fn timestamp(value: &str) -> Option<i64> {
    let value = value.trim();
    let (date, clock) = value.split_once(' ').unwrap_or((value, "00:00"));
    let date = Date::parse(
        &date.replace('/', "-"),
        format_description!("[year]-[month]-[day]"),
    )
    .ok()?;
    let clock = Time::parse(
        clock,
        format_description!("[hour]:[minute][optional [:[second]]]"),
    )
    .ok()?;
    Some(date.with_time(clock).as_utc().unix_timestamp())
}

pub fn date_label(timestamp: i64) -> String {
    UtcDateTime::from_unix_timestamp(timestamp)
        .map(|dt| dt.date().to_string())
        .unwrap_or_default()
}

pub fn series(snapshot: &Snapshot) -> Vec<RankPoint> {
    let account = snapshot.username.trim().to_lowercase();
    if account.is_empty() {
        return Vec::new();
    }
    let mut points: Vec<_> = snapshot
        .games
        .iter()
        .filter_map(|game| {
            let mut ranks = [&game.black, &game.white].into_iter().filter_map(|raw| {
                let (name, rank) = player(raw);
                if raw.trim().to_lowercase() == account {
                    Some("—") // The whole field is the account name, including any rank-like suffix.
                } else if name.to_lowercase() == account {
                    Some(rank)
                } else {
                    None
                }
            });
            let rank = ranks.next()?;
            if ranks.any(|other| other != rank) {
                return None;
            }
            Some(RankPoint {
                level: level(rank, snapshot.server)?,
                timestamp: timestamp(&game.date)?,
            })
        })
        .collect();
    points.sort_by_key(|point| point.timestamp);
    points
}
