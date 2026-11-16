//! Turning driver values into grid text (same approach as Quervio).

use mssql_tds::datatypes::column_values::{
    ColumnValues, SqlDateTime, SqlDateTime2, SqlDateTimeOffset, SqlSmallDateTime, SqlTime,
};

/// `SqlTime::time_nanoseconds` holds 100 ns ticks, whatever the column scale.
const TICKS_PER_SECOND: u64 = 10_000_000;

/// Days from 0001-01-01 to 1900-01-01 (`datetime` / `smalldatetime` epoch).
const DAYS_TO_1900: i64 = 693_595;

/// Appends display text of `value` to `out`; `None` for SQL NULL.
pub fn write_value(value: &ColumnValues, out: &mut String) -> Option<()> {
    use std::fmt::Write;
    match value {
        ColumnValues::Null => return None,
        ColumnValues::TinyInt(v) => write!(out, "{v}"),
        ColumnValues::SmallInt(v) => write!(out, "{v}"),
        ColumnValues::Int(v) => write!(out, "{v}"),
        ColumnValues::BigInt(v) => write!(out, "{v}"),
        ColumnValues::Real(v) => write!(out, "{v}"),
        ColumnValues::Float(v) => write!(out, "{v}"),
        ColumnValues::Decimal(v) | ColumnValues::Numeric(v) => write!(out, "{v}"),
        ColumnValues::Bit(v) => write!(out, "{}", u8::from(*v)),
        ColumnValues::String(v) => write!(out, "{}", v.to_utf8_string()),
        ColumnValues::Money(v) => {
            let scaled = (i64::from(v.msb_part) << 32) | i64::from(v.lsb_part as u32);
            write_money(i128::from(scaled), out);
            Ok(())
        }
        ColumnValues::SmallMoney(v) => {
            write_money(i128::from(v.int_val), out);
            Ok(())
        }
        ColumnValues::Date(v) => {
            write_date(i64::from(v.get_days()), out);
            Ok(())
        }
        ColumnValues::DateTime(v) => {
            write_datetime(v, out);
            Ok(())
        }
        ColumnValues::SmallDateTime(v) => {
            write_smalldatetime(v, out);
            Ok(())
        }
        ColumnValues::DateTime2(v) => {
            write_datetime2(v, out);
            Ok(())
        }
        ColumnValues::DateTimeOffset(v) => {
            write_datetimeoffset(v, out);
            Ok(())
        }
        ColumnValues::Time(v) => {
            write_time(v, out);
            Ok(())
        }
        ColumnValues::Uuid(v) => {
            let mut buffer = [0; 36];
            out.push_str(v.hyphenated().encode_upper(&mut buffer));
            Ok(())
        }
        ColumnValues::Bytes(bytes) => {
            write_hex(bytes, out);
            Ok(())
        }
        ColumnValues::Xml(v) => write!(out, "{}", v.as_string()),
        ColumnValues::Json(v) => write!(out, "{}", v.as_string()),
        ColumnValues::Vector(v) => match v.as_f32() {
            Some(values) => write!(out, "{values:?}"),
            None => write!(out, "{v:?}"),
        },
    }
    .ok()
}

fn write_hex(bytes: &[u8], out: &mut String) {
    const DIGITS: &[u8; 16] = b"0123456789ABCDEF";
    out.reserve(2 + bytes.len() * 2);
    out.push_str("0x");
    for &byte in bytes {
        out.push(DIGITS[usize::from(byte >> 4)] as char);
        out.push(DIGITS[usize::from(byte & 0xF)] as char);
    }
}

fn write_money(scaled: i128, out: &mut String) {
    use std::fmt::Write;
    let sign = if scaled < 0 { "-" } else { "" };
    let abs = scaled.unsigned_abs();
    let _ = write!(out, "{sign}{}.{:04}", abs / 10_000, abs % 10_000);
}

fn civil_from_days(days_since_0001: i64) -> (i64, u32, u32) {
    let z = days_since_0001 + 306;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

fn write_date(days_since_0001: i64, out: &mut String) {
    use std::fmt::Write;
    let (year, month, day) = civil_from_days(days_since_0001);
    let _ = write!(out, "{year:04}-{month:02}-{day:02}");
}

fn write_clock(seconds: u64, out: &mut String) {
    use std::fmt::Write;
    let _ = write!(
        out,
        "{:02}:{:02}:{:02}",
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60
    );
}

fn write_time(time: &SqlTime, out: &mut String) {
    use std::fmt::Write;
    let ticks = time.time_nanoseconds;
    write_clock(ticks / TICKS_PER_SECOND, out);
    if time.scale > 0 {
        let fraction = ticks % TICKS_PER_SECOND / 10u64.pow(7 - u32::from(time.scale.min(7)));
        let _ = write!(out, ".{fraction:0width$}", width = usize::from(time.scale));
    }
}

fn write_datetime(value: &SqlDateTime, out: &mut String) {
    use std::fmt::Write;
    write_date(i64::from(value.days) + DAYS_TO_1900, out);
    out.push(' ');
    let millis = (u64::from(value.time) * 10 + 1) / 3;
    write_clock(millis / 1000, out);
    let _ = write!(out, ".{:03}", millis % 1000);
}

fn write_smalldatetime(value: &SqlSmallDateTime, out: &mut String) {
    write_date(i64::from(value.days) + DAYS_TO_1900, out);
    out.push(' ');
    write_clock(u64::from(value.time) * 60, out);
}

fn write_datetime2(value: &SqlDateTime2, out: &mut String) {
    write_date(i64::from(value.days), out);
    out.push(' ');
    write_time(&value.time, out);
}

fn write_datetimeoffset(value: &SqlDateTimeOffset, out: &mut String) {
    use std::fmt::Write;
    let ticks_per_second = i128::from(TICKS_PER_SECOND);
    let offset = i128::from(value.offset) * 60 * ticks_per_second;
    let day = 86_400 * ticks_per_second;
    let utc =
        i128::from(value.datetime2.days) * day + i128::from(value.datetime2.time.time_nanoseconds);
    let local = utc + offset;
    let local = SqlDateTime2 {
        days: local.div_euclid(day) as u32,
        time: SqlTime {
            time_nanoseconds: local.rem_euclid(day) as u64,
            scale: value.datetime2.time.scale,
        },
    };
    write_datetime2(&local, out);
    let sign = if value.offset < 0 { '-' } else { '+' };
    let minutes = value.offset.unsigned_abs();
    let _ = write!(out, " {sign}{:02}:{:02}", minutes / 60, minutes % 60);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(value: ColumnValues) -> Option<String> {
        let mut out = String::new();
        write_value(&value, &mut out).map(|()| out)
    }

    #[test]
    fn simple_values() {
        assert_eq!(text(ColumnValues::Null), None);
        assert_eq!(text(ColumnValues::Bit(true)).as_deref(), Some("1"));
        assert_eq!(text(ColumnValues::Int(-5)).as_deref(), Some("-5"));
        assert_eq!(
            text(ColumnValues::Bytes(vec![0, 171, 255])).as_deref(),
            Some("0x00ABFF")
        );
    }
}
