//! Browser `Date.toString()` formatting for the prepare `p` config (slot 1).
//!
//! Ported from v1 `channels/chatgpt/prepare_p.rs` (the date helpers). The output
//! must match a real browser's `new Date().toString()` so the 25-slot config the
//! server reconstructs looks browser-shaped.

/// Format a unix-seconds timestamp as a browser `Date.toString()` string, e.g.
/// `"Tue Apr 21 2026 17:25:57 GMT+0800 (中国标准时间)"`.
pub fn format_browser_date(unix_secs: i64, offset_minutes: i32, zone_label: &str) -> String {
    let shifted = unix_secs + (offset_minutes as i64) * 60;
    let weekdays = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    let months = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let (y, m, d, hh, mm, ss, wd) = unix_to_ymd(shifted);
    let sign = if offset_minutes >= 0 { '+' } else { '-' };
    let abs = offset_minutes.abs();
    format!(
        "{wday} {mon} {day:02} {year} {hh:02}:{mm:02}:{ss:02} GMT{sign}{oh:02}{om:02} ({zone})",
        wday = weekdays[wd as usize],
        mon = months[(m - 1) as usize],
        day = d,
        year = y,
        hh = hh,
        mm = mm,
        ss = ss,
        sign = sign,
        oh = abs / 60,
        om = abs % 60,
        zone = zone_label,
    )
}

/// Convert unix seconds to (year, month[1-12], day, hour, min, sec, weekday[0=Sun]).
/// Handles only dates after 1970-01-01.
pub fn unix_to_ymd(secs: i64) -> (i32, u32, u32, u32, u32, u32, u32) {
    let days = secs.div_euclid(86400);
    let time = secs.rem_euclid(86400) as u32;
    let hh = time / 3600;
    let mm = (time % 3600) / 60;
    let ss = time % 60;
    // 1970-01-01 was a Thursday (weekday 4)
    let wd = ((days + 4).rem_euclid(7)) as u32;
    // Civil from days algorithm (Howard Hinnant)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = (z - era * 146_097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y as i32, m, d, hh, mm, ss, wd)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unix_to_ymd_known_dates() {
        assert_eq!(unix_to_ymd(0), (1970, 1, 1, 0, 0, 0, 4));
        // 2026-04-21 00:00:00 UTC -> unix 1776729600
        assert_eq!(unix_to_ymd(1_776_729_600), (2026, 4, 21, 0, 0, 0, 2));
    }
}
