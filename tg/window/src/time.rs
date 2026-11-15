//! Dates as the window writes them, from Unix seconds and the zone's offset the backend
//! gives: no clock and no zone table of its own.

use alloc::format;
use alloc::string::String;

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];
const DAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

/// A moment in a zone's civil time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Civil {
    pub year: i64,
    /// 1 to 12.
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    /// 0 is Monday.
    pub weekday: u32,
    /// Days since 1970-01-01 in that zone.
    pub days: i64,
}

/// Days since 1970-01-01 of a civil date (Howard Hinnant's `days_from_civil`).
pub const fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = (m as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The civil time of `unix` seconds in a zone `offset` seconds east of UTC.
pub fn civil(unix: i64, offset: i32) -> Civil {
    let t = unix + i64::from(offset);
    let days = t.div_euclid(86_400);
    let secs = t.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    Civil {
        year,
        month,
        day,
        hour: (secs / 3600) as u32,
        minute: (secs % 3600 / 60) as u32,
        weekday: (days + 3).rem_euclid(7) as u32,
        days,
    }
}

/// `14:05`.
pub fn clock(unix: i64, offset: i32) -> String {
    let c = civil(unix, offset);
    format!("{:02}:{:02}", c.hour, c.minute)
}

/// A chat list row's time: the clock today, the weekday within a week, else the date.
pub fn list_time(unix: i64, now: i64, offset: i32) -> String {
    let (c, n) = (civil(unix, offset), civil(now, offset));
    match n.days - c.days {
        0 => format!("{:02}:{:02}", c.hour, c.minute),
        1..=6 => String::from(DAYS[c.weekday as usize]),
        _ if c.year == n.year => format!("{} {}", c.day, &MONTHS[c.month as usize - 1][..3]),
        _ => format!("{:02}.{:02}.{}", c.day, c.month, c.year),
    }
}

/// The line between two days of a chat: `Today`, `Yesterday`, `14 November`,
/// `3 March 2025`.
pub fn day_label(unix: i64, now: i64, offset: i32) -> String {
    let (c, n) = (civil(unix, offset), civil(now, offset));
    match n.days - c.days {
        0 => String::from("Today"),
        1 => String::from("Yesterday"),
        _ if c.year == n.year => format!("{} {}", c.day, MONTHS[c.month as usize - 1]),
        _ => format!("{} {} {}", c.day, MONTHS[c.month as usize - 1], c.year),
    }
}
