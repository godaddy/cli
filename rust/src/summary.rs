use cli_engine::{NextAction, PaginationMeta};
use serde::Serialize;

/// Envelope for an list of data that may be paginated
#[derive(Debug, Clone, Serialize)]
pub(crate) struct Summary<T: Serialize> {
    pub(crate) items: Vec<T>,
    pub(crate) pagination: PaginationMeta,
}

impl<T: Serialize> Summary<T> {
    /// Truncates `all` to at most `cap` items, recording how many were cut
    /// as a `PaginationMeta` (offset is always 0 — an embedded preview
    /// always starts from the top; there's no local `--offset` to apply).
    pub(crate) fn capped(mut all: Vec<T>, cap: usize) -> Self {
        let total = all.len();
        let truncated = total > cap;
        if truncated {
            all.truncate(cap);
        }
        let shown = all.len();
        let pagination = PaginationMeta {
            total: total as i64,
            offset: 0,
            limit: cap as i64,
            count: shown as i64,
            has_more: truncated,
        };
        Self {
            items: all,
            pagination,
        }
    }

    /// Returns `action` only when this summary was actually truncated, so no
    /// call site has to repeat `if truncated { vec![...] } else { vec![] }`.
    pub(crate) fn next_action_if_truncated(&self, action: NextAction) -> Vec<NextAction> {
        if self.pagination.has_more {
            vec![action]
        } else {
            vec![]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Summary;
    use crate::next_action::next_action;

    #[test]
    fn under_cap_is_not_truncated() {
        let summary = Summary::capped(vec![1, 2, 3], 5);
        assert_eq!(summary.pagination.total, 3);
        assert_eq!(summary.pagination.count, 3);
        assert!(!summary.pagination.has_more);
        assert_eq!(summary.items, vec![1, 2, 3]);
    }

    #[test]
    fn exactly_at_cap_is_not_truncated() {
        let summary = Summary::capped(vec![1, 2, 3], 3);
        assert_eq!(summary.pagination.total, 3);
        assert_eq!(summary.pagination.count, 3);
        assert!(!summary.pagination.has_more);
    }

    #[test]
    fn over_cap_is_truncated_and_items_are_cut() {
        let summary = Summary::capped(vec![1, 2, 3, 4, 5], 3);
        assert_eq!(summary.pagination.total, 5);
        assert_eq!(summary.pagination.count, 3);
        assert!(summary.pagination.has_more);
        assert_eq!(summary.items, vec![1, 2, 3]);
    }

    #[test]
    fn next_action_if_truncated_is_empty_when_untruncated() {
        let summary = Summary::capped(vec![1, 2, 3], 5);
        let actions = summary.next_action_if_truncated(next_action("api foo", "see more"));
        assert!(actions.is_empty());
    }

    #[test]
    fn next_action_if_truncated_carries_the_action_when_truncated() {
        let summary = Summary::capped(vec![1, 2, 3], 2);
        let actions = summary.next_action_if_truncated(next_action("api foo", "see more"));
        assert_eq!(actions.len(), 1);
    }

    #[test]
    fn pagination_reflects_an_untruncated_summary() {
        let summary = Summary::capped(vec![1, 2, 3], 5);
        assert_eq!(summary.pagination.total, 3);
        assert_eq!(summary.pagination.offset, 0);
        assert_eq!(summary.pagination.limit, 5);
        assert_eq!(summary.pagination.count, 3);
        assert!(!summary.pagination.has_more);
    }

    #[test]
    fn pagination_reflects_a_truncated_summary() {
        let summary = Summary::capped(vec![1, 2, 3, 4, 5], 3);
        assert_eq!(summary.pagination.total, 5);
        assert_eq!(summary.pagination.offset, 0);
        assert_eq!(summary.pagination.limit, 3);
        assert_eq!(summary.pagination.count, 3);
        assert!(summary.pagination.has_more);
    }
}
