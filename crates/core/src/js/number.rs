//! `Number.prototype.toString()` for base 10 (ECMA-262 `Number::toString`).

/// The text JavaScript's `String(value)` gives a number. `NaN` and the infinities are spelled out
/// as JavaScript does; `JSON.stringify` writes those as `null` instead (see `stringify`).
pub fn number_to_string(value: f64) -> String {
    if value.is_nan() {
        return "NaN".to_owned();
    }
    if value == 0.0 {
        return "0".to_owned();
    }
    if value.is_infinite() {
        return if value > 0.0 { "Infinity" } else { "-Infinity" }.to_owned();
    }
    let sign = if value < 0.0 { "-" } else { "" };
    // `{:e}` prints the fewest digits that read back as the same double, which is how many
    // JavaScript uses. When two strings of that length both read back and are equally close to
    // the exact value, JavaScript takes the even one, which `{:e}` doesn't always do. Printing
    // the exact value correctly rounded to that many digits does.
    let shortest = format!("{:e}", value.abs());
    let count = shortest.split_once('e').map_or(shortest.len(), |(m, _)| {
        m.chars().filter(char::is_ascii_digit).count()
    });
    let scientific = format!("{:.*e}", count.saturating_sub(1), value.abs());
    let (mantissa, exponent) = scientific
        .split_once('e')
        .unwrap_or((scientific.as_str(), "0"));
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let exponent: i32 = exponent.parse().unwrap_or(0);
    let k = i32::try_from(digits.len()).unwrap_or(i32::MAX);
    // value = 0.digits × 10^n
    let n = exponent.saturating_add(1);
    format!("{sign}{}", layout(&digits, k, n))
}

fn layout(digits: &str, k: i32, n: i32) -> String {
    let zeros = |count: i32| "0".repeat(usize::try_from(count).unwrap_or(0));
    if k <= n && n <= 21 {
        return format!("{digits}{}", zeros(n - k));
    }
    if 0 < n && n <= 21 {
        let split = usize::try_from(n).unwrap_or(0);
        let (int, frac) = digits.split_at(split.min(digits.len()));
        return format!("{int}.{frac}");
    }
    if -6 < n && n <= 0 {
        return format!("0.{}{digits}", zeros(-n));
    }
    let e = n - 1;
    let sign = if e < 0 { '-' } else { '+' };
    let mut chars = digits.chars();
    let first = chars.next().unwrap_or('0');
    let rest: String = chars.collect();
    if rest.is_empty() {
        format!("{first}e{sign}{}", e.unsigned_abs())
    } else {
        format!("{first}.{rest}e{sign}{}", e.unsigned_abs())
    }
}

#[cfg(test)]
mod tests {
    use super::number_to_string;

    #[test]
    fn matches_javascript_for_the_usual_cases() {
        let cases = [
            (0.0, "0"),
            (-0.0, "0"),
            (1.0, "1"),
            (-1.5, "-1.5"),
            (0.1 + 0.2, "0.30000000000000004"),
            (100.0, "100"),
            (1e21, "1e+21"),
            (1e20, "100000000000000000000"),
            (1e-6, "0.000001"),
            (1e-7, "1e-7"),
            (1.5e-7, "1.5e-7"),
            (123_456_789.125, "123456789.125"),
            // A tie between two shortest digit strings: JavaScript takes the even one.
            (f64::from_bits(0xc2ee_5d93_a6d3_0904), "-267098063804488.12"),
            (5e-324, "5e-324"),
            (f64::MAX, "1.7976931348623157e+308"),
            (f64::NAN, "NaN"),
            (f64::NEG_INFINITY, "-Infinity"),
        ];
        for (value, expected) in cases {
            assert_eq!(number_to_string(value), expected, "{value:e}");
        }
    }
}

/// `Number(text)`: JavaScript's string-to-number conversion. Surrounding whitespace is
/// ignored, an empty string is 0, `0x`/`0o`/`0b` prefixes are read, and anything else that
/// isn't a decimal literal or `Infinity` is NaN.
pub fn string_to_number(text: &str) -> f64 {
    let text = text.trim_matches(crate::html::is_js_whitespace);
    if text.is_empty() {
        return 0.0;
    }
    for (prefix, radix) in [
        ("0x", 16),
        ("0X", 16),
        ("0o", 8),
        ("0O", 8),
        ("0b", 2),
        ("0B", 2),
    ] {
        if let Some(digits) = text.strip_prefix(prefix) {
            if digits.is_empty() || !digits.chars().all(|c| c.is_digit(radix)) {
                return f64::NAN;
            }
            return digits.chars().fold(0.0, |n, c| {
                n * f64::from(radix) + f64::from(c.to_digit(radix).unwrap_or(0))
            });
        }
    }
    let unsigned = text.strip_prefix(['+', '-']).unwrap_or(text);
    if unsigned == "Infinity" {
        return if text.starts_with('-') {
            f64::NEG_INFINITY
        } else {
            f64::INFINITY
        };
    }
    // A decimal literal: digits with an optional fraction (or a fraction alone), then an
    // optional exponent.
    let bytes = unsigned.as_bytes();
    let mut i = 0;
    let digits = |i: &mut usize| {
        let start = *i;
        while *i < bytes.len() && bytes[*i].is_ascii_digit() {
            *i += 1;
        }
        *i - start
    };
    let whole = digits(&mut i);
    let mut fraction = 0;
    if i < bytes.len() && bytes[i] == b'.' {
        i += 1;
        fraction = digits(&mut i);
    }
    if whole == 0 && fraction == 0 {
        return f64::NAN;
    }
    if i < bytes.len() && (bytes[i] == b'e' || bytes[i] == b'E') {
        i += 1;
        if i < bytes.len() && (bytes[i] == b'+' || bytes[i] == b'-') {
            i += 1;
        }
        if digits(&mut i) == 0 {
            return f64::NAN;
        }
    }
    if i != bytes.len() {
        return f64::NAN;
    }
    text.parse().unwrap_or(f64::NAN)
}

#[cfg(test)]
mod string_tests {
    use super::string_to_number;

    #[test]
    fn reads_strings_as_javascript_does() {
        for (text, n) in [
            ("", 0.0),
            (" 20 ", 20.0),
            ("0x10", 16.0),
            ("1e2", 100.0),
            ("-.5", -0.5),
            ("5.", 5.0),
            ("-Infinity", f64::NEG_INFINITY),
        ] {
            assert_eq!(string_to_number(text), n, "{text:?}");
        }
        for text in ["abc", "1px", ".", "e5", "1e", "-0x1", "inf", "1_0"] {
            assert!(string_to_number(text).is_nan(), "{text:?}");
        }
    }
}
