use std::time::{SystemTime, UNIX_EPOCH};

use super::clock::{Clock, SystemClock};

#[test]
fn system_clock_is_within_a_few_seconds_of_the_os_time() {
    let os = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;

    let ours = SystemClock.now_unix();

    assert!((ours - os).abs() <= 5, "ours={ours} os={os}");
}
