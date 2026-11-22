use katastro::archive::period::{Bucket, Interval, Period};
use time::{Date, macros::date};

// Classify the played dates from all three archive formats into comparable,
// disjoint periods. A leap day must count once and both boundaries must be usable.
fn partition(period: Period, today: Date, dates: &[&str]) -> Vec<Option<Bucket>> {
    let windows = period.windows(today);
    dates.iter().map(|s| windows.bucket(s)).collect()
}

#[test]
fn rolling_and_calendar_presets_partition_played_dates() {
    use Bucket::{Current as C, Previous as P};
    let today = date!(2024 - 03 - 01);
    assert_eq!(
        partition(
            Period::Last30,
            today,
            &[
                "2024/01/01 23:59",
                "2024-01-02",
                "2024/01/31",
                "2024-02-01",
                "2024/02/29 21:10",
                "2024-03-01 23:59:59",
                "2024-03-02",
                "pending",
            ]
        ),
        [
            None,
            Some(P),
            Some(P),
            Some(C),
            Some(C),
            Some(C),
            None,
            None
        ]
    );
    assert_eq!(
        partition(
            Period::default(),
            today,
            &[
                "2023-09-03",
                "2023-09-04",
                "2023-12-02",
                "2023-12-03",
                "2024-03-01"
            ]
        ),
        [None, Some(P), Some(P), Some(C), Some(C)]
    );
    assert_eq!(
        partition(
            Period::ThisYear,
            today,
            &[
                "2023-10-31",
                "2023-11-01",
                "2023-12-31",
                "2024-01-01",
                "2024-03-01",
                "2024-03-02"
            ]
        ),
        [None, Some(P), Some(P), Some(C), Some(C), None]
    );
}

#[test]
fn a_custom_leap_day_compares_with_the_day_before_and_all_time_restores_undated_games() {
    use Bucket::{Current as C, Previous as P};
    let day = date!(2024 - 02 - 29);
    let dates = [
        "2024/02/27",
        "2024-02-28",
        "2024/02/29 23:59",
        "2024-03-01",
        "unknown",
        "2023/02/29",
    ];
    assert_eq!(
        partition(
            Period::Custom(Interval::new(day, day).unwrap()),
            date!(2026 - 01 - 01),
            &dates
        ),
        [None, Some(P), Some(C), None, None, None]
    );
    assert!(Interval::new(date!(2024 - 03 - 01), day).is_none());
    assert_eq!(partition(Period::AllTime, day, &dates), [Some(C); 6]);
    assert!(Period::AllTime.windows(day).previous.is_none());
}

#[test]
fn local_records_count_decided_games_on_each_side_of_hour_and_weekday_boundaries() {
    use katastro::archive::play_times::PlayTimes;

    let mut times = PlayTimes::default();
    for (played, won) in [
        ("2024/03/03 23:59", Some(false)), // Sunday
        ("2024/03/04 00:00", Some(true)),  // Monday
        ("2024/03/04 05:59", Some(false)),
        ("2024/03/04 06:00", Some(false)),
        ("2024/03/04 11:59:59", Some(true)),
        ("2024/03/04 12:00", Some(true)),
        ("2024/03/04 17:59", Some(true)),
        ("2024/03/04 18:00", Some(false)),
        ("2024-03-04", Some(true)),
        ("", Some(false)),
        ("2024/03/04 06:05", None),
    ] {
        times.count(played, won);
    }
    let monday = times.weekdays[0];
    assert_eq!(
        (monday.wins, monday.losses, monday.win_rate()),
        (5, 3, Some(62.5))
    );
    assert_eq!(times.weekdays[6].losses, 1);
    assert_eq!(times.weekdays[1].win_rate(), None);
    assert_eq!((times.hours[0].wins, times.hours[23].losses), (1, 1));
    assert_eq!(
        times.six_hour_blocks().map(|r| (r.wins, r.losses)),
        [(1, 1), (1, 1), (2, 0), (0, 2)]
    );
    assert_eq!((times.unknown_time.wins, times.unknown_time.losses), (1, 1));
    assert_eq!((times.unknown_day.wins, times.unknown_day.losses), (0, 1));
    assert_eq!(times.unknown_outcome, 1);
}
