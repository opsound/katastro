//! Recorded wins and losses grouped by local play dates and the player's color.
use super::{categories::Evidence, period::play_date_time};
use crate::Color;

#[derive(Clone, Copy, Default)]
pub struct Record {
    pub wins: usize,
    pub losses: usize,
}

impl Record {
    fn count(&mut self, won: bool) {
        if won {
            self.wins += 1;
        } else {
            self.losses += 1;
        }
    }

    pub fn win_rate(self) -> Option<f64> {
        let played = self.wins + self.losses;
        (played > 0).then(|| 100.0 * self.wins as f64 / played as f64)
    }
}

#[derive(Default)]
pub struct PlayTimes {
    pub weekdays: [Record; 7],
    pub hours: [Record; 24],
    pub unknown_day: Record,
    pub unknown_time: Record,
    pub unknown_outcome: usize,
    pub as_black: Record,
    pub as_white: Record,
}

impl PlayTimes {
    pub fn count_game(&mut self, played: &str, evidence: Evidence) {
        let won = evidence.outcome().map(|(player, won)| {
            match player {
                Color::Black => &mut self.as_black,
                Color::White => &mut self.as_white,
            }
            .count(won);
            won
        });
        self.count(played, won);
    }

    pub fn overall(&self) -> Record {
        self.weekdays
            .iter()
            .fold(self.unknown_day, |total, day| Record {
                wins: total.wins + day.wins,
                losses: total.losses + day.losses,
            })
    }

    pub fn count(&mut self, played: &str, won: Option<bool>) {
        let Some(won) = won else {
            self.unknown_outcome += 1;
            return;
        };
        let (weekday, hour) = match play_date_time(played) {
            Some((date, clock)) => (
                &mut self.weekdays[usize::from(date.weekday().number_days_from_monday())],
                match clock {
                    Some(time) => &mut self.hours[usize::from(time.hour())],
                    None => &mut self.unknown_time,
                },
            ),
            None => (&mut self.unknown_day, &mut self.unknown_time),
        };
        for record in [weekday, hour] {
            record.count(won);
        }
    }

    pub fn six_hour_blocks(&self) -> [Record; 4] {
        let mut blocks = [Record::default(); 4];
        for (hour, record) in self.hours.iter().enumerate() {
            blocks[hour / 6].wins += record.wins;
            blocks[hour / 6].losses += record.losses;
        }
        blocks
    }
}
