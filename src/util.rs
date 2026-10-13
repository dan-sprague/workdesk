//! Small date helpers. Dates are stored as `YYYY-MM-DD` strings and timestamps as
//! RFC 3339 UTC strings, the same shapes the Work Desk page uses.

use std::time::{SystemTime, UNIX_EPOCH};

/// Days since 1970-01-01 for a civil date (Howard Hinnant's algorithm).
pub fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = (if y >= 0 { y } else { y - 399 }) / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

pub fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let era = (if z >= 0 { z } else { z - 146096 }) / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

pub fn iso_from_days(days: i64) -> String {
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Parse `YYYY-MM-DD` (anything after the date is ignored) into days since epoch.
pub fn parse_date(s: &str) -> Option<i64> {
    let s = s.get(0..10)?;
    let mut it = s.split('-');
    let y: i64 = it.next()?.parse().ok()?;
    let m: i64 = it.next()?.parse().ok()?;
    let d: i64 = it.next()?.parse().ok()?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) || s.as_bytes()[4] != b'-' || s.as_bytes()[7] != b'-' {
        return None;
    }
    let days = days_from_civil(y, m, d);
    // reject impossible dates such as Feb 30
    if iso_from_days(days) != s {
        return None;
    }
    Some(days)
}

/// Seconds since epoch for an RFC 3339 UTC timestamp like `2026-10-13T15:33:00Z`.
pub fn parse_ts(s: &str) -> Option<i64> {
    let days = parse_date(s)?;
    let t = s.get(11..19).unwrap_or("00:00:00");
    let mut it = t.split(':');
    let h: i64 = it.next()?.parse().ok()?;
    let mi: i64 = it.next()?.parse().ok()?;
    let se: i64 = it.next().and_then(|x| x.parse().ok()).unwrap_or(0);
    Some(days * 86400 + h * 3600 + mi * 60 + se)
}

pub fn now_secs() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

pub fn now_iso() -> String {
    let s = now_secs();
    let (y, m, d) = civil_from_days(s.div_euclid(86400));
    let r = s.rem_euclid(86400);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", r / 3600, (r / 60) % 60, r % 60)
}

/// Today's date on this computer's clock and time zone.
pub fn today_days() -> i64 {
    let t = now_secs() as libc::time_t;
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    let ok = unsafe { !libc::localtime_r(&t, &mut tm).is_null() };
    if ok {
        days_from_civil(tm.tm_year as i64 + 1900, tm.tm_mon as i64 + 1, tm.tm_mday as i64)
    } else {
        now_secs().div_euclid(86400)
    }
}

pub fn today_iso() -> String {
    iso_from_days(today_days())
}

pub fn weekday_name(days: i64) -> &'static str {
    ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"][(days + 4).rem_euclid(7) as usize]
}

/// "Oct 31"
pub fn short_date(iso: &str) -> String {
    match parse_date(iso) {
        Some(d) => {
            let (_, m, day) = civil_from_days(d);
            let mon = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"][(m - 1) as usize];
            format!("{mon} {day}")
        }
        None => iso.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn roundtrip() {
        for d in [-1000, 0, 1, 20_000, 20_739] {
            assert_eq!(parse_date(&iso_from_days(d)), Some(d));
        }
        assert_eq!(iso_from_days(0), "1970-01-01");
        assert_eq!(weekday_name(0), "Thursday");
        assert_eq!(weekday_name(parse_date("2026-10-13").unwrap()), "Tuesday");
        assert_eq!(parse_date("2026-02-30"), None);
        assert_eq!(parse_ts("1970-01-02T00:00:10Z"), Some(86410));
    }
}
