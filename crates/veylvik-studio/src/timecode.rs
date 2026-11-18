//! Timecode for display and typing: frames as hours:minutes:seconds:frames at the composition's rate. 29.97 and 59.94 use drop-frame counting (shown with a semicolon), so the clock matches elapsed time; other fractional rates count at their nominal rate.
use veylvik_document::Time;

/// How positions in time are shown.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Display {
    #[default]
    Timecode,
    Frames,
}
/// Frames counted in a second of timecode, and the frame numbers skipped at the start
/// of each minute that is not a tenth (0 without drop-frame).
fn counting(rate: Time) -> (i64, i64) {
    let fps = rate.ticks as f64 / f64::from(rate.rate.max(1));
    let nominal = (fps.round() as i64).max(1);
    let drop = rate.rate == 1001 && matches!(rate.ticks, 30000 | 60000);
    (nominal, if drop { nominal / 15 } else { 0 })
}
/// `frame` as HH:MM:SS:FF (HH:MM:SS;FF in drop-frame).
pub fn format(frame: i64, rate: Time) -> String {
    let (nominal, dropped) = counting(rate);
    let mut number = frame.unsigned_abs() as i64;
    if dropped > 0 {
        // Put back the numbers drop-frame skips, so the labels run on the clock.
        let ten_minutes = nominal * 600 - 9 * dropped;
        let minute = nominal * 60 - dropped;
        let within = number % ten_minutes;
        number += 9 * dropped * (number / ten_minutes);
        if within > dropped {
            number += dropped * ((within - dropped) / minute);
        }
    }
    format!(
        "{}{:02}:{:02}:{:02}{}{:02}",
        if frame < 0 { "-" } else { "" },
        number / (nominal * 3600),
        number / (nominal * 60) % 60,
        number / nominal % 60,
        if dropped > 0 { ';' } else { ':' },
        number % nominal
    )
}
/// A shorter label for rulers: leading zero hours and minutes are left out.
pub fn short(frame: i64, rate: Time) -> String {
    let full = format(frame, rate);
    let trimmed = full.strip_prefix("00:").unwrap_or(&full);
    trimmed.strip_prefix("00:").unwrap_or(trimmed).to_owned()
}
/// The frame a typed position means: a timecode (any of HH:MM:SS:FF, MM:SS:FF, SS:FF),
/// or a plain frame number.
pub fn parse(text: &str, rate: Time) -> Option<i64> {
    let text = text.trim();
    let (negative, text) = text
        .strip_prefix('-')
        .map_or((false, text), |rest| (true, rest));
    let parts: Vec<i64> = text
        .split([':', ';', '.'])
        .map(|part| part.trim().parse::<i64>().ok().filter(|v| *v >= 0))
        .collect::<Option<_>>()?;
    let sign = if negative { -1 } else { 1 };
    let (nominal, dropped) = counting(rate);
    let [hours, minutes, seconds, frames] = match parts[..] {
        [frames] => return Some(sign * frames),
        [seconds, frames] => [0, 0, seconds, frames],
        [minutes, seconds, frames] => [0, minutes, seconds, frames],
        [hours, minutes, seconds, frames] => [hours, minutes, seconds, frames],
        _ => return None,
    };
    let total_minutes = hours.checked_mul(60)?.checked_add(minutes)?;
    let mut number = total_minutes
        .checked_mul(60)?
        .checked_add(seconds)?
        .checked_mul(nominal)?
        .checked_add(frames)?;
    if dropped > 0 {
        number -= dropped * (total_minutes - total_minutes / 10);
    }
    Some(sign * number)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn rate(ticks: i64, per: u32) -> Time {
        Time::new(ticks, per).unwrap()
    }
    #[test]
    fn whole_rates_count_plainly_and_typing_round_trips() {
        let r = rate(24, 1);
        assert_eq!(format(0, r), "00:00:00:00");
        assert_eq!(format(23, r), "00:00:00:23");
        assert_eq!(format(24, r), "00:00:01:00");
        assert_eq!(format(24 * 3661 + 5, r), "01:01:01:05");
        assert_eq!(format(-30, r), "-00:00:01:06");
        assert_eq!(short(24 * 75 + 3, r), "01:15:03");
        assert_eq!(short(50, r), "02:02");
        for frame in [0, 1, 23, 24, 1439, 1440, 86_399, 86_400, 2_073_600] {
            assert_eq!(parse(&format(frame, r), r), Some(frame));
        }
        // Shorter forms and plain frame numbers.
        assert_eq!(parse("5:12", r), Some(5 * 24 + 12));
        assert_eq!(parse("1:05:00", r), Some(65 * 24));
        assert_eq!(parse("  240 ", r), Some(240));
        assert_eq!(parse("-12", r), Some(-12));
        assert_eq!(parse("1:2:3:4:5", r), None);
        assert_eq!(parse("ten", r), None);
        assert_eq!(parse("", r), None);
    }
    #[test]
    fn drop_frame_matches_the_clock_and_skips_the_right_numbers() {
        let r = rate(30000, 1001);
        assert_eq!(format(0, r), "00:00:00;00");
        assert_eq!(format(29, r), "00:00:00;29");
        // The first minute ends at frame 1799; numbers ;00 and ;01 are skipped.
        assert_eq!(format(1799, r), "00:00:59;29");
        assert_eq!(format(1800, r), "00:01:00;02");
        // Every tenth minute keeps them.
        assert_eq!(format(17_981, r), "00:09:59;29");
        assert_eq!(format(17_982, r), "00:10:00;00");
        // One hour of real time is 107892 frames, and reads as one hour.
        assert_eq!(format(107_892, r), "01:00:00;00");
        for frame in [
            0, 1, 1799, 1800, 1801, 17_981, 17_982, 17_983, 107_891, 107_892, 215_784,
        ] {
            assert_eq!(parse(&format(frame, r), r), Some(frame), "{frame}");
        }
        let double = rate(60000, 1001);
        assert_eq!(format(3599, double), "00:00:59;59");
        assert_eq!(format(3600, double), "00:01:00;04");
        assert_eq!(format(215_784, double), "01:00:00;00");
        // 23.976 counts at 24 without dropping.
        let film = rate(24000, 1001);
        assert_eq!(format(24, film), "00:00:01:00");
        assert_eq!(parse("00:00:01:00", film), Some(24));
    }
}
