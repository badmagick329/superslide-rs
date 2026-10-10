//! Modified times as the owner's wall clock. Windows applies the time zone's rules for that date,
//! daylight saving included, which `std` cannot do.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use windows_sys::Win32::Foundation::{FILETIME, SYSTEMTIME};
use windows_sys::Win32::System::Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime};

use crate::info_overlay::domain::LocalTime;

/// 100 ns ticks from 1601-01-01, where `FILETIME` starts, to the Unix epoch.
const UNIX_EPOCH_TICKS: u64 = 116_444_736_000_000_000;

/// `None` for a time Windows cannot convert.
pub fn local_time(time: SystemTime) -> Option<LocalTime> {
    let utc = utc(time)?;
    let mut local = zeroed();
    // A null zone means the one Windows is set to now.
    let ok = unsafe { SystemTimeToTzSpecificLocalTime(std::ptr::null(), &utc, &mut local) };
    (ok != 0).then(|| minute(local))
}

fn utc(time: SystemTime) -> Option<SYSTEMTIME> {
    let ticks = |span: Duration| u64::try_from(span.as_nanos() / 100).ok();
    // A camera with an unset clock can stamp files before 1970.
    let ticks = match time.duration_since(UNIX_EPOCH) {
        Ok(after) => UNIX_EPOCH_TICKS.checked_add(ticks(after)?),
        Err(before) => UNIX_EPOCH_TICKS.checked_sub(ticks(before.duration())?),
    }?;
    let file_time = FILETIME {
        dwLowDateTime: ticks as u32,
        dwHighDateTime: (ticks >> 32) as u32,
    };
    let mut utc = zeroed();
    let ok = unsafe { FileTimeToSystemTime(&file_time, &mut utc) };
    (ok != 0).then_some(utc)
}

fn zeroed() -> SYSTEMTIME {
    SYSTEMTIME {
        wYear: 0,
        wMonth: 0,
        wDayOfWeek: 0,
        wDay: 0,
        wHour: 0,
        wMinute: 0,
        wSecond: 0,
        wMilliseconds: 0,
    }
}

fn minute(t: SYSTEMTIME) -> LocalTime {
    LocalTime {
        year: t.wYear,
        month: t.wMonth as u8,
        day: t.wDay as u8,
        hour: t.wHour as u8,
        minute: t.wMinute as u8,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_unix_time_lands_on_its_utc_calendar_minute() {
        // 2026-03-09 08:05:59 UTC.
        let time = UNIX_EPOCH + Duration::from_secs(1_773_043_559);
        let expected = LocalTime {
            year: 2026,
            month: 3,
            day: 9,
            hour: 8,
            minute: 5,
        };
        assert_eq!(utc(time).map(minute), Some(expected));
    }

    #[test]
    fn the_local_time_is_within_a_day_of_utc() {
        let time = UNIX_EPOCH + Duration::from_secs(1_773_043_559);
        let local = local_time(time).unwrap();
        assert_eq!((local.year, local.month), (2026, 3));
        assert!((8..=10).contains(&local.day), "{local:?}");
    }

    #[test]
    fn times_before_1970_still_have_a_date() {
        let time = UNIX_EPOCH - Duration::from_secs(60);
        let expected = LocalTime {
            year: 1969,
            month: 12,
            day: 31,
            hour: 23,
            minute: 59,
        };
        assert_eq!(utc(time).map(minute), Some(expected));
    }
}
