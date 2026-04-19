use super::types::ASSUMED_CM_PER_SCROLL_STEP;

pub fn euclidean_distance(x: f64, y: f64) -> f64 {
    (x * x + y * y).sqrt()
}

pub fn counts_to_centimeters(counts: f64, dpi: f64) -> f64 {
    if dpi <= 0.0 {
        return 0.0;
    }

    counts / dpi * 2.54
}

pub fn relative_counts_to_centimeters(dx: f64, dy: f64, dpi: f64) -> f64 {
    counts_to_centimeters(euclidean_distance(dx, dy), dpi)
}

#[cfg(target_os = "windows")]
pub fn millimeters_to_centimeters(dx_mm: f64, dy_mm: f64) -> f64 {
    euclidean_distance(dx_mm, dy_mm) / 10.0
}

pub fn scroll_steps_to_centimeters(steps: f64) -> f64 {
    steps.abs() * ASSUMED_CM_PER_SCROLL_STEP
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verifies that relative motion conversion uses Euclidean distance by passing a
    /// 3-4-5 triangle and comparing against the expected hypotenuse-based conversion.
    #[test]
    fn relative_counts_to_centimeters_uses_euclidean_distance() {
        let distance_cm = relative_counts_to_centimeters(3.0, 4.0, 800.0);
        assert!((distance_cm - (5.0 / 800.0 * 2.54)).abs() < 1e-6);
    }

    /// Verifies that scroll conversion uses absolute step count by passing a negative step
    /// value and asserting the result matches the positive travel distance.
    #[test]
    fn scroll_steps_to_centimeters_uses_absolute_step_count() {
        assert!((scroll_steps_to_centimeters(-2.0) - 0.8).abs() < 1e-6);
    }
}
