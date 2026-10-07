//! Optional depth-source admission, independent of graphics backend creation.
pub(crate) const DEPTH_BUDGET_BYTES: u64 = 64 * 1024 * 1024;
pub(crate) const LOCAL_DEPTH_BUDGET_BYTES: u64 = 32 * 1024 * 1024;
pub(crate) const METADATA_BUDGET_BYTES: usize = 4 * 1024 * 1024;
const IDLE_UPDATES: u32 = 60;

pub(crate) fn depth_bytes(resolution: u32) -> Option<u64> {
    u64::from(resolution)
        .checked_mul(u64::from(resolution))?
        .checked_mul(4)
}
pub(crate) fn depth_admitted(resolution: u32, max_dimension: u32, budget: u64) -> bool {
    resolution > 1
        && resolution <= max_dimension
        && depth_bytes(resolution).is_some_and(|bytes| bytes <= budget)
}
/// A static source's certificate holds one bit per draw plus its uniform row.
/// The 4 MiB metadata budget therefore admits about 33 million draws.
pub(crate) fn certificate_admitted(draws: usize, row_bytes: usize) -> bool {
    draws
        .div_ceil(64)
        .checked_mul(8)
        .and_then(|bytes| bytes.checked_add(row_bytes))
        .and_then(|bytes| bytes.checked_add(64))
        .is_some_and(|bytes| bytes <= METADATA_BUDGET_BYTES)
}
pub(crate) fn retained_depth_bytes(
    used: u64,
    bytes: u64,
    budget: u64,
    enabled: bool,
) -> Option<u64> {
    if !enabled {
        return None;
    }
    used.checked_add(bytes).filter(|total| *total <= budget)
}
pub(crate) fn next_idle_age(age: u32) -> Option<u32> {
    age.checked_add(1).filter(|next| *next < IDLE_UPDATES)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn depth_admission_bounds_bytes_dimensions_and_checked_arithmetic() {
        assert_eq!(depth_bytes(1024), Some(4 * 1024 * 1024));
        assert_eq!(depth_bytes(4096), Some(DEPTH_BUDGET_BYTES));
        assert!(depth_admitted(4096, 8192, DEPTH_BUDGET_BYTES));
        assert!(!depth_admitted(8192, 8192, DEPTH_BUDGET_BYTES));
        assert!(!depth_admitted(4096, 2048, DEPTH_BUDGET_BYTES));
        assert!(!depth_admitted(1024, 8192, 0));
        assert!(!depth_admitted(0, 8192, DEPTH_BUDGET_BYTES));
        assert!(!depth_admitted(1, 8192, DEPTH_BUDGET_BYTES));
        assert_eq!(depth_bytes(u32::MAX), None);
    }
    #[test]
    fn certificate_admission_bounds_bits_rows_and_overflow() {
        // The former 16,384-key cap no longer limits static sources.
        assert!(certificate_admitted(16_385, 80));
        assert!(certificate_admitted(1_000_000, 80));
        assert!(certificate_admitted(33_000_000, 80));
        assert!(!certificate_admitted(34_000_000, 80));
        assert!(!certificate_admitted(1, METADATA_BUDGET_BYTES));
        assert!(!certificate_admitted(usize::MAX, 0));
        assert!(!certificate_admitted(1, usize::MAX));
    }
    #[test]
    fn local_depth_budget_preserves_default_faces_and_rejects_larger_layers() {
        let mut spots = 0;
        for _ in 0..8 {
            spots = retained_depth_bytes(
                spots,
                depth_bytes(1024).unwrap(),
                LOCAL_DEPTH_BUDGET_BYTES,
                true,
            )
            .unwrap();
        }
        assert_eq!(spots, LOCAL_DEPTH_BUDGET_BYTES);
        assert!(retained_depth_bytes(spots, 1, LOCAL_DEPTH_BUDGET_BYTES, true).is_none());
        let mut points = 0;
        for _ in 0..24 {
            points = retained_depth_bytes(
                points,
                depth_bytes(512).unwrap(),
                LOCAL_DEPTH_BUDGET_BYTES,
                true,
            )
            .unwrap();
        }
        assert_eq!(points, 24 * 1024 * 1024);
        assert!(spots + points + DEPTH_BUDGET_BYTES <= 128 * 1024 * 1024);
        assert!(
            retained_depth_bytes(
                0,
                depth_bytes(4096).unwrap(),
                LOCAL_DEPTH_BUDGET_BYTES,
                true
            )
            .is_none()
        );
        assert!(retained_depth_bytes(0, 1, LOCAL_DEPTH_BUDGET_BYTES, false).is_none());
        assert!(retained_depth_bytes(u64::MAX, 1, u64::MAX, true).is_none());
    }
    #[test]
    fn intermittent_movers_keep_a_bounded_idle_grace() {
        let mut age = 0;
        for update in 1..60 {
            age = next_idle_age(age).unwrap();
            assert_eq!(age, update);
        }
        assert!(next_idle_age(age).is_none());
        assert!(next_idle_age(u32::MAX).is_none());
        age = 0;
        assert_eq!(next_idle_age(age), Some(1));
    }
}
