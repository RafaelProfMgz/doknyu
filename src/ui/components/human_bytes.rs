//! Byte counts for people: `1536` is "1.5 KiB".

const UNITS: [&str; 5] = ["KiB", "MiB", "GiB", "TiB", "PiB"];
const STEP: f64 = 1024.0;

/// `n` bytes with a binary unit and one decimal; plain bytes below 1 KiB.
pub fn human_bytes(n: u64) -> String {
    if n < 1024 {
        return format!("{n} B");
    }
    let mut value = n as f64;
    let mut unit = "B";
    for next in UNITS {
        value /= STEP;
        unit = next;
        if value < STEP {
            break;
        }
    }
    format!("{value:.1} {unit}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_bytes_formats() {
        assert_eq!(human_bytes(0), "0 B");
        assert_eq!(human_bytes(1023), "1023 B");
        assert_eq!(human_bytes(1024), "1.0 KiB");
        assert_eq!(human_bytes(1536), "1.5 KiB");
        assert_eq!(human_bytes(5 * 1024 * 1024), "5.0 MiB");
        assert_eq!(human_bytes(3 * 1024 * 1024 * 1024), "3.0 GiB");
        // The largest value neither overflows nor runs out of units.
        assert!(human_bytes(u64::MAX).ends_with("PiB"));
    }
}
