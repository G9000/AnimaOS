//! Pure planning guards for exact-image reversible recurring-work repairs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecurringPauseCandidate {
    pub enabled: bool,
    pub recurring: bool,
    pub workspace_target: bool,
    pub has_verification: bool,
    pub completed: bool,
    pub consecutive_failures: u64,
}
impl RecurringPauseCandidate {
    pub fn eligible(&self) -> bool {
        self.enabled
            && self.recurring
            && self.workspace_target
            && !self.has_verification
            && !self.completed
            && self.consecutive_failures >= 3
    }
}
pub fn exact_image(current: &str, expected: &str) -> bool {
    expected.len() == 64
        && expected
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        && current == expected
}
pub fn may_undo(current: &str, after: &str, already_undone: bool) -> bool {
    !already_undone && exact_image(current, after)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn guarded_repair_requires_a_failing_recurring_work_and_exact_image() {
        let candidate = RecurringPauseCandidate {
            enabled: true,
            recurring: true,
            workspace_target: true,
            has_verification: false,
            completed: false,
            consecutive_failures: 3,
        };
        assert!(candidate.eligible());
        for n in 0..6 {
            let mut changed = candidate;
            match n {
                0 => changed.enabled = false,
                1 => changed.recurring = false,
                2 => changed.workspace_target = false,
                3 => changed.has_verification = true,
                4 => changed.completed = true,
                _ => changed.consecutive_failures = 2,
            }
            assert!(!changed.eligible())
        }
        let digest = "a".repeat(64);
        assert!(exact_image(&digest, &digest));
        assert!(!exact_image(&digest, &"b".repeat(64)));
        assert!(!exact_image("bad", "bad"));
        assert!(may_undo(&digest, &digest, false));
        assert!(!may_undo(&digest, &digest, true));
    }
}
