//! Java text conversions shared by registry and client resource binding.

use crate::java_character;

/// JDK 25 Integer.parseInt decimal semantics, including BMP decimal digits.
#[inline]
pub fn parse_java_integer(text: &str) -> Option<i32> {
    text.parse().ok().or_else(|| {
        if text.is_ascii() {
            return None;
        }
        parse_java_decimal(text, i128::from(i32::MAX)).map(|value| value as i32)
    })
}

/// JDK 25 Long.parseLong decimal semantics, including BMP decimal digits.
#[inline]
pub fn parse_java_long(text: &str) -> Option<i64> {
    text.parse().ok().or_else(|| {
        if text.is_ascii() {
            return None;
        }
        parse_java_decimal(text, i128::from(i64::MAX)).map(|value| value as i64)
    })
}

fn parse_java_decimal(text: &str, max: i128) -> Option<i128> {
    let (negative, text) = if let Some(text) = text.strip_prefix('-') {
        (true, text)
    } else {
        (false, text.strip_prefix('+').unwrap_or(text))
    };
    if text.is_empty() {
        return None;
    }
    let mut value = 0i128;
    for c in text.chars() {
        let digit = java_digit(c, 10)?;
        value = value * 10 + i128::from(digit);
        if value > max + i128::from(negative) {
            return None;
        }
    }
    Some(if negative { -value } else { value })
}

/// `Character.digit(char, radix)`: the char overload sees no supplementary scalar.
fn java_digit(c: char, radix: u32) -> Option<u32> {
    java_character::digit(u16::try_from(u32::from(c)).ok()?, radix)
}

/// Decode eight ASCII hex bytes in parallel. Each packed byte starts below 128,
/// so adding the splatted range offsets cannot carry into the next byte.
#[inline]
fn uuid_hex_word(bytes: [u8; 8]) -> Option<u32> {
    const BYTE: u64 = 0x0101_0101_0101_0101;
    const HIGH: u64 = BYTE * 128;
    let n = u64::from_be_bytes(bytes);
    if n & HIGH != 0 {
        return None;
    }
    let lower = n | (BYTE * u64::from(b' '));
    let decimal = ((n + BYTE * (128 - b'0' as u64)) ^ (n + BYTE * (127 - b'9' as u64))) & HIGH;
    let alpha =
        ((lower + BYTE * (128 - b'a' as u64)) ^ (lower + BYTE * (127 - b'f' as u64))) & HIGH;
    if decimal | alpha != HIGH {
        return None;
    }
    let nibbles = (n & (BYTE * 15)) + ((alpha >> 7) * 9);
    let pairs = (nibbles | (nibbles >> 4)) & 0x00ff_00ff_00ff_00ff;
    let quads = (pairs | (pairs >> 8)) & 0x0000_ffff_0000_ffff;
    Some((quads | (quads >> 16)) as u32)
}

/// `UUID.fromString`: signed-long hex groups, masked to UUID field widths.
pub fn parse_java_uuid(name: &str) -> Option<u128> {
    if let Ok(&b) = <&[u8; 36]>::try_from(name.as_bytes())
        && b[8] == b'-'
        && b[13] == b'-'
        && b[18] == b'-'
        && b[23] == b'-'
        && let (Some(a), Some(c), Some(d), Some(e)) = (
            uuid_hex_word(b[..8].try_into().unwrap()),
            uuid_hex_word([b[9], b[10], b[11], b[12], b[14], b[15], b[16], b[17]]),
            uuid_hex_word([b[19], b[20], b[21], b[22], b[24], b[25], b[26], b[27]]),
            uuid_hex_word(b[28..].try_into().unwrap()),
        )
    {
        return Some(
            u128::from(a) << 96 | u128::from(c) << 64 | u128::from(d) << 32 | u128::from(e),
        );
    }
    if name.encode_utf16().count() > 36 {
        return None;
    }
    let mut parts = name.split('-');
    let mut values = [0u64; 5];
    for value in &mut values {
        let group = parts.next()?;
        let digits = group.strip_prefix('+').unwrap_or(group);
        if digits.is_empty() {
            return None;
        }
        for digit in digits.chars() {
            *value = value
                .checked_mul(16)?
                .checked_add(u64::from(java_digit(digit, 16)?))?;
            if *value > i64::MAX as u64 {
                return None;
            }
        }
    }
    if parts.next().is_some() {
        return None;
    }
    let most =
        ((values[0] & 0xffff_ffff) << 32) | ((values[1] & 0xffff) << 16) | (values[2] & 0xffff);
    let least = ((values[3] & 0xffff) << 48) | (values[4] & 0xffff_ffff_ffff);
    Some((u128::from(most) << 64) | u128::from(least))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_uuid_accepts_mixed_case_hex_but_not_colon() {
        assert_eq!(
            parse_java_uuid("01234567-89Ab-cDeF-fEDc-bA9876543210"),
            Some(0x0123_4567_89ab_cdef_fedc_ba98_7654_3210)
        );
        assert_eq!(
            parse_java_uuid("01234567-89:b-cDeF-fEDc-bA9876543210"),
            None
        );
    }

    #[test]
    fn decimal_integer_and_long_boundaries_follow_java() {
        for (text, expected) in [
            ("2147483647", Some(i32::MAX)),
            ("-2147483648", Some(i32::MIN)),
            ("2147483648", None),
            ("-2147483649", None),
            ("+\u{0661}\u{ff12}", Some(12)),
        ] {
            assert_eq!(parse_java_integer(text), expected, "{text}");
        }
        for (text, expected) in [
            ("9223372036854775807", Some(i64::MAX)),
            ("-9223372036854775808", Some(i64::MIN)),
            ("9223372036854775808", None),
            ("-9223372036854775809", None),
            ("+\u{0661}\u{ff12}", Some(12)),
        ] {
            assert_eq!(parse_java_long(text), expected, "{text}");
        }
        for text in [
            "",
            "+",
            "-",
            " 12",
            "12 ",
            "1_2",
            "\u{b2}",
            "\u{2460}",
            "\u{1d7cf}",
            "\u{2212}1",
        ] {
            assert_eq!(parse_java_integer(text), None, "{text}");
            assert_eq!(parse_java_long(text), None, "{text}");
        }
        let zeroes = format!("{}12", "0".repeat(100));
        assert_eq!(parse_java_integer(&zeroes), Some(12));
        assert_eq!(parse_java_long(&zeroes), Some(12));
        assert_eq!(parse_java_long(&"9".repeat(100)), None);
    }
}
