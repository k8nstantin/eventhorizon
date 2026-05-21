//! Outcomes returned by write operations.

use serde::{Deserialize, Serialize};

/// What a successful `execute_append` returns.
///
/// Phase 1 reports just the row count. Phase 7+ may extend with returned
/// generated columns (e.g., a server-side default that the gateway should
/// surface back to the caller); add as additional fields with serde
/// defaults so existing clients are not broken.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppendOutcome {
    /// Number of rows inserted. For Phase 1 this is always 1; bulk-append
    /// is a Phase 7+ feature.
    pub rows_inserted: u64,
}

impl AppendOutcome {
    /// One row inserted.
    #[must_use]
    pub const fn one() -> Self {
        Self { rows_inserted: 1 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_is_one() {
        assert_eq!(AppendOutcome::one().rows_inserted, 1);
    }

    #[test]
    fn round_trip() {
        let o = AppendOutcome::one();
        let s = serde_json::to_string(&o).unwrap();
        let back: AppendOutcome = serde_json::from_str(&s).unwrap();
        assert_eq!(o, back);
    }
}
