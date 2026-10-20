//! Bounded content checks adapted from MIT-licensed source;
//! see the repository's NOTICE for attribution and scope.
//! These detect runaway output, never tool progress or effect safety.

use std::collections::{HashMap, HashSet, VecDeque};

pub(super) const REPETITION_ERROR: &str = "model reply interrupted after runaway repetition";
pub(super) const REPETITION_INTERRUPTED: &str =
    "[the reply degenerated into a repetition loop and was interrupted]";
const MIN_RUNAWAY_CHARS: usize = 16_000;
const TAIL_CHARS: usize = 4 * MIN_RUNAWAY_CHARS;
const WINDOW: usize = 60;
const MIN_REPEATS: usize = 5;

#[derive(Default)]
pub(super) struct RunawayStreamWatch {
    tail: VecDeque<char>,
    chars: usize,
    next_check: usize,
}

impl RunawayStreamWatch {
    pub(super) fn feed(&mut self, text: &str) -> bool {
        if self.next_check == 0 {
            self.next_check = MIN_RUNAWAY_CHARS;
        }
        for ch in text.chars() {
            self.tail.push_back(ch);
            if self.tail.len() > TAIL_CHARS {
                self.tail.pop_front();
            }
            self.chars = self.chars.saturating_add(1);
            if self.chars >= self.next_check {
                self.next_check = self.chars.saturating_add(self.chars.min(TAIL_CHARS));
                if runaway(&self.tail.iter().copied().collect::<Vec<_>>()) {
                    return true;
                }
            }
        }
        false
    }
}

pub(super) fn is_runaway(text: &str) -> bool {
    let mut tail: Vec<char> = text.chars().rev().take(TAIL_CHARS).collect();
    tail.reverse();
    runaway(&tail)
}

fn runaway(text: &[char]) -> bool {
    if text.len() < MIN_RUNAWAY_CHARS {
        return false;
    }
    let rendered: String = text.iter().collect();
    let lines: Vec<&str> = rendered
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    // Distinct batch rows can share long prefixes without being a loop.
    if lines.len() >= MIN_REPEATS
        && lines.iter().copied().collect::<HashSet<_>>().len() * 2 > lines.len()
        && lines
            .iter()
            .all(|line| line.chars().count().saturating_mul(2) <= text.len())
    {
        return false;
    }
    let mut line_counts = HashMap::new();
    for line in lines {
        let count = line_counts.entry(line).or_insert(0usize);
        *count += 1;
        if *count >= MIN_REPEATS && *count * line.chars().count() * 2 >= text.len() {
            return true;
        }
    }
    let needed = MIN_REPEATS.max(text.len().div_ceil(WINDOW * 2));
    let mut windows = HashMap::new();
    for window in text.windows(WINDOW) {
        let count = windows.entry(window).or_insert(0usize);
        *count += 1;
        if *count >= needed {
            return true;
        }
    }
    // Bounded anchor and candidate-period sampling catches long loop units
    // whose individual windows repeat infrequently.
    let max_start = text.len() - WINDOW;
    let step = max_start.div_ceil(31).max(1);
    let mut rejected = Vec::new();
    for start in (0..max_start)
        .step_by(step)
        .chain(std::iter::once(max_start))
    {
        let mut search_from = start + 1;
        for _ in 0..8 {
            let Some(relative) = text[search_from..]
                .windows(WINDOW)
                .position(|window| window == &text[start..start + WINDOW])
            else {
                break;
            };
            let found = search_from + relative;
            let period = found - start;
            search_from = found + 1;
            if rejected.iter().any(|&(left, right, old_period)| {
                left <= start && start < right && period % old_period == 0
            }) {
                continue;
            }
            let mut left = start;
            while left > 0 && text[left - 1] == text[left - 1 + period] {
                left -= 1;
            }
            let mut right = start + WINDOW;
            while right + period < text.len() && text[right] == text[right + period] {
                right += 1;
            }
            right += period;
            if right - left >= MIN_REPEATS * period && (right - left) * 2 >= text.len() {
                return true;
            }
            rejected.push((left, right, period));
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distinct_intro_does_not_hide_a_dominant_single_line_loop() {
        let text = format!(
            "one\ntwo\nthree\nfour\nfive\n{}",
            "looped phrase ".repeat(2000)
        );
        assert!(is_runaway(&text));
        assert!(RunawayStreamWatch::default().feed(&text));
    }
}
