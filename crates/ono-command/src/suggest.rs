//! Near-miss suggestions for names that did not resolve (spec §15.4, ADR-0011).
//!
//! The search runs only on the failure path, so a name that exists costs nothing.

/// The candidate closest to `typed`, when one is close enough to be worth offering.
///
/// "Close enough" is a third of the typed length, rounded up and never less than one, which keeps
/// `prcoess` → `process` while refusing to pair two unrelated short words.
pub(crate) fn closest<'a>(
    typed: &str,
    candidates: impl IntoIterator<Item = &'a str>,
) -> Option<&'a str> {
    if typed.is_empty() {
        return None;
    }
    let budget = typed.chars().count().div_ceil(3).max(1);
    candidates
        .into_iter()
        .map(|candidate| (distance(typed, candidate), candidate))
        .filter(|(distance, _)| *distance <= budget)
        .min_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(right.1)))
        .map(|(_, candidate)| candidate)
}

/// The Levenshtein distance between two names, over characters rather than bytes.
fn distance(left: &str, right: &str) -> usize {
    let right_chars: Vec<char> = right.chars().collect();
    let mut previous: Vec<usize> = (0..=right_chars.len()).collect();
    let mut current = vec![0; right_chars.len() + 1];

    for (row, left_char) in left.chars().enumerate() {
        current[0] = row + 1;
        for (column, right_char) in right_chars.iter().enumerate() {
            let substitution = previous[column] + usize::from(left_char != *right_char);
            let deletion = previous[column + 1] + 1;
            let insertion = current[column] + 1;
            current[column + 1] = substitution.min(deletion).min(insertion);
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[right_chars.len()]
}

#[cfg(test)]
mod tests {
    use super::{closest, distance};

    #[test]
    fn should_measure_the_edits_between_two_names() {
        assert_eq!(distance("process", "process"), 0);
        assert_eq!(distance("prcoess", "process"), 2);
        assert_eq!(distance("", "get"), 3);
    }

    #[test]
    fn should_offer_the_nearest_candidate_within_budget() {
        assert_eq!(closest("prcoess", ["process", "service"]), Some("process"));
        assert_eq!(closest("zzzzzzzz", ["process", "service"]), None);
        assert_eq!(closest("", ["process"]), None);
    }
}
