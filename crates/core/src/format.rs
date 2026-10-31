//! Number formatting for export.

/// `v` with `precision` significant digits, like C's `%.{precision}g`: fixed notation for moderate exponents,
/// scientific otherwise, and trailing zeros removed.
pub fn format_g(v: f64, precision: usize) -> String {
    let precision = precision.max(1);
    if v == 0.0 {
        return if v.is_sign_negative() { "-0".into() } else { "0".into() };
    }
    if !v.is_finite() {
        return if v.is_nan() {
            "nan".into()
        } else if v > 0.0 {
            "inf".into()
        } else {
            "-inf".into()
        };
    }
    let sci = format!("{:.*e}", precision - 1, v);
    let (mantissa, exp) = sci.split_once('e').expect("scientific format has an exponent");
    let exp: i32 = exp.parse().expect("exponent is an integer");
    if exp < -4 || exp >= precision as i32 {
        let sign = if exp < 0 { '-' } else { '+' };
        format!("{}e{}{:02}", trim_zeros(mantissa), sign, exp.abs())
    } else {
        let decimals = (precision as i32 - 1 - exp).max(0) as usize;
        trim_zeros(&format!("{v:.decimals$}")).to_string()
    }
}

fn trim_zeros(s: &str) -> &str {
    if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.') } else { s }
}

#[cfg(test)]
mod tests {
    use super::format_g;

    #[test]
    fn matches_c_printf_g() {
        for (v, want) in [
            (0.0, "0"),
            (1.0, "1"),
            (-2.5, "-2.5"),
            (123456.789, "123456.789"),
            (0.0001, "0.0001"),
            (0.00001234, "1.234e-05"),
            (1e10, "1e+10"),
            (12345678901.0, "1.23456789e+10"),
            (0.1 + 0.2, "0.3"),
            (std::f64::consts::PI, "3.141592654"),
            (99999.99999999999, "100000"),
        ] {
            assert_eq!(format_g(v, 10), want, "{v}");
        }
    }
}
