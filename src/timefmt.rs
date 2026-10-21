use time::macros::format_description;
use time::{Duration, OffsetDateTime, UtcOffset};

fn month_day(t: OffsetDateTime) -> String {
    t.format(format_description!("[month repr:short] [day padding:none]"))
        .expect("format description is valid")
}

pub fn relative(t: OffsetDateTime, now: OffsetDateTime, offset: UtcOffset) -> String {
    let diff = now - t;
    if diff < Duration::minutes(1) {
        "now".into()
    } else if diff < Duration::hours(1) {
        format!("{}m", diff.whole_minutes())
    } else if diff < Duration::days(1) {
        format!("{}h", diff.whole_hours())
    } else if diff <= Duration::days(7) {
        format!("{}d", diff.whole_days())
    } else {
        let local = t.to_offset(offset);
        if local.year() == now.to_offset(offset).year() {
            month_day(local)
        } else {
            format!("{}, {}", month_day(local), local.year())
        }
    }
}

pub fn full(t: OffsetDateTime, offset: UtcOffset) -> String {
    t.to_offset(offset)
        .format(format_description!(
            "[month repr:short] [day padding:none], [year] [hour]:[minute]"
        ))
        .expect("format description is valid")
}

pub fn day_label(t: OffsetDateTime, now: OffsetDateTime, offset: UtcOffset) -> String {
    let local = t.to_offset(offset);
    let today = now.to_offset(offset).date();
    if local.date() == today {
        "Today".into()
    } else if today.previous_day() == Some(local.date()) {
        "Yesterday".into()
    } else {
        local
            .format(format_description!(
                "[weekday repr:short], [month repr:short] [day padding:none]"
            ))
            .expect("format description is valid")
    }
}

#[cfg(test)]
mod tests {
    use time::Duration;
    use time::macros::{datetime, offset};

    use super::*;

    const NOW: OffsetDateTime = datetime!(2026-10-20 12:00:00 UTC);

    fn rel(ago: Duration) -> String {
        relative(NOW - ago, NOW, offset!(UTC))
    }

    // 11. relative

    #[test]
    fn t11_under_a_minute_is_now() {
        assert_eq!(rel(Duration::seconds(0)), "now");
        assert_eq!(rel(Duration::seconds(59)), "now");
    }

    #[test]
    fn t11_minutes_hours_days_boundaries() {
        assert_eq!(rel(Duration::seconds(60)), "1m");
        assert_eq!(rel(Duration::minutes(59)), "59m");
        assert_eq!(rel(Duration::minutes(60)), "1h");
        assert_eq!(rel(Duration::hours(23)), "23h");
        assert_eq!(rel(Duration::hours(24)), "1d");
        assert_eq!(rel(Duration::days(7)), "7d");
    }

    #[test]
    fn t11_over_seven_days_shows_date_without_year_in_same_year() {
        assert_eq!(rel(Duration::days(7) + Duration::seconds(1)), "Oct 13");
    }

    #[test]
    fn t11_previous_year_shows_date_with_year() {
        let t = datetime!(2025-10-20 12:00:00 UTC);
        assert_eq!(relative(t, NOW, offset!(UTC)), "Oct 20, 2025");
    }

    #[test]
    fn t11_future_is_now() {
        assert_eq!(rel(Duration::seconds(-300)), "now");
    }

    #[test]
    fn t11_year_is_compared_in_local_offset_when_utc_years_differ() {
        let t = datetime!(2025-12-31 16:00:00 UTC);
        let now = datetime!(2026-01-20 00:00:00 UTC);
        assert_eq!(relative(t, now, offset!(+9)), "Jan 1");
    }

    #[test]
    fn t11_year_differs_locally_when_utc_years_are_equal() {
        let t = datetime!(2026-01-01 02:00:00 UTC);
        let now = datetime!(2026-01-10 12:00:00 UTC);
        assert_eq!(relative(t, now, offset!(-5)), "Dec 31, 2025");
    }

    // 12. full

    #[test]
    fn t12_full_renders_in_given_offset() {
        let t = datetime!(2026-10-20 14:02:00 UTC);
        assert_eq!(full(t, offset!(+9)), "Oct 20, 2026 23:02");
    }

    // 13. day_label

    #[test]
    fn t13_same_day_is_today() {
        let t = datetime!(2026-10-20 01:00:00 UTC);
        assert_eq!(day_label(t, NOW, offset!(UTC)), "Today");
    }

    #[test]
    fn t13_previous_day_is_yesterday() {
        let t = datetime!(2026-10-19 23:00:00 UTC);
        assert_eq!(day_label(t, NOW, offset!(UTC)), "Yesterday");
    }

    #[test]
    fn t13_older_is_weekday_month_day() {
        let t = datetime!(2026-10-17 10:00:00 UTC);
        assert_eq!(day_label(t, NOW, offset!(UTC)), "Sat, Oct 17");
    }

    #[test]
    fn t13_utc_yesterday_can_be_today_in_local_offset() {
        let t = datetime!(2026-10-19 20:00:00 UTC);
        let now = datetime!(2026-10-20 01:00:00 UTC);
        assert_eq!(day_label(t, now, offset!(UTC)), "Yesterday");
        assert_eq!(day_label(t, now, offset!(+9)), "Today");
    }
}
