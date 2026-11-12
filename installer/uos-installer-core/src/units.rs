// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

//! Byte units, alignment and size formatting.

/// One kibibyte in bytes.
pub const KIB: u64 = 1024;
/// One mebibyte in bytes.
pub const MIB: u64 = 1024 * KIB;
/// One gibibyte in bytes.
pub const GIB: u64 = 1024 * MIB;
/// One tebibyte in bytes.
pub const TIB: u64 = 1024 * GIB;

/// Rounds `value` down to a multiple of `alignment`.
///
/// Returns `None` when `alignment` is zero.
pub fn align_down(value: u64, alignment: u64) -> Option<u64> {
    value
        .checked_rem(alignment)
        .map(|remainder| value - remainder)
}

/// Rounds `value` up to a multiple of `alignment`.
///
/// Returns `None` when `alignment` is zero or the result does not fit in 64 bits.
pub fn align_up(value: u64, alignment: u64) -> Option<u64> {
    value.checked_next_multiple_of(alignment)
}

/// Formats a byte count with a binary unit and one decimal place, for example
/// `931.5 GiB`. The value is truncated, not rounded, so it never overstates.
pub fn format_size(bytes: u64) -> String {
    const UNITS: [(&str, u64); 4] = [("TiB", TIB), ("GiB", GIB), ("MiB", MIB), ("KiB", KIB)];
    for (name, unit) in UNITS {
        if bytes >= unit {
            let tenths = u128::from(bytes) * 10 / u128::from(unit);
            return format!("{}.{} {name}", tenths / 10, tenths % 10);
        }
    }
    format!("{bytes} B")
}

/// Formats a byte count as a truncated size followed by the exact number of bytes.
pub fn format_size_exact(bytes: u64) -> String {
    format!("{} ({bytes} bytes)", format_size(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aligns_down_and_up() {
        assert_eq!(align_down(0, MIB), Some(0));
        assert_eq!(align_down(MIB + 1, MIB), Some(MIB));
        assert_eq!(align_down(2 * MIB - 1, MIB), Some(MIB));
        assert_eq!(align_up(0, MIB), Some(0));
        assert_eq!(align_up(1, MIB), Some(MIB));
        assert_eq!(align_up(MIB, MIB), Some(MIB));
        assert_eq!(align_up(MIB + 1, MIB), Some(2 * MIB));
    }

    #[test]
    fn alignment_never_panics_on_extremes() {
        assert_eq!(align_down(5, 0), None);
        assert_eq!(align_up(5, 0), None);
        assert_eq!(align_up(u64::MAX, MIB), None);
        assert_eq!(align_down(u64::MAX, MIB), Some(u64::MAX - (MIB - 1)));
    }

    #[test]
    fn formats_sizes_without_overstating() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(1023), "1023 B");
        assert_eq!(format_size(KIB), "1.0 KiB");
        assert_eq!(format_size(GIB), "1.0 GiB");
        assert_eq!(format_size(GIB + GIB / 2), "1.5 GiB");
        assert_eq!(format_size(2 * GIB - 1), "1.9 GiB");
        assert_eq!(format_size(1_000_204_886_016), "931.5 GiB");
        assert_eq!(format_size(u64::MAX), "16777215.9 TiB");
        assert_eq!(format_size_exact(MIB), "1.0 MiB (1048576 bytes)");
    }
}
