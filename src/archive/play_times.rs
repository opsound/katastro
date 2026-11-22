//! Wins and losses grouped by the library's local play dates.
use super::period::play_date_time;

#[derive(Clone, Copy, Default)]
pub struct Record {
    pub wins: usize,
    pub losses: usize,
}

impl Record {
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
}

impl PlayTimes {
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
            if won {
                record.wins += 1;
            } else {
                record.losses += 1;
            }
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
