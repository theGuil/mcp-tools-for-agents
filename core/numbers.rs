//! Formatação de números no estilo do Python, usada em nomes de arquivo e
//! expressões de filtro do ffmpeg.

/// Equivalente do `f"{value:g}"` do Python: seis dígitos significativos, sem
/// zeros à direita, notação científica só para valores muito grandes ou pequenos.
///
/// `0.5` vira `"0.5"`, `2.0` vira `"2"`, `1234567.0` vira `"1.23457e+06"`.
pub fn format_g(value: f64) -> String {
    if value == 0.0 {
        return "0".to_string();
    }
    if !value.is_finite() {
        return if value.is_nan() {
            "nan".to_string()
        } else if value > 0.0 {
            "inf".to_string()
        } else {
            "-inf".to_string()
        };
    }
    const PRECISION: i32 = 6;
    // Arredonda para seis dígitos significativos antes de decidir a notação.
    let scientific = format!("{value:.*e}", (PRECISION - 1) as usize);
    let (mantissa, exponent) = scientific.split_once('e').unwrap_or((&scientific, "0"));
    let exponent: i32 = exponent.parse().unwrap_or(0);
    if !(-4..PRECISION).contains(&exponent) {
        let mantissa = strip_zeros(mantissa);
        let sign = if exponent < 0 { '-' } else { '+' };
        return format!("{mantissa}e{sign}{:02}", exponent.abs());
    }
    let decimals = (PRECISION - 1 - exponent).max(0) as usize;
    strip_zeros(&format!("{value:.decimals$}"))
}

fn strip_zeros(text: &str) -> String {
    if text.contains('.') {
        text.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        text.to_string()
    }
}

/// Arredonda para `decimals` casas, como `round(value, decimals)` do Python.
pub fn round_to(value: f64, decimals: u32) -> f64 {
    let factor = 10f64.powi(decimals as i32);
    (value * factor).round() / factor
}
