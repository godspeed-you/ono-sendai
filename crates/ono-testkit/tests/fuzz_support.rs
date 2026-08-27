//! The deterministic fuzzing helper. Spec §35.6 requires fuzzing of every parser and decoder,
//! and AGENTS.md §11 requires every test to be deterministic. Those are only compatible if the
//! randomness is reproducible, so the helper is tested for reproducibility itself.

use ono_testkit::Rng;

#[test]
fn should_produce_the_same_sequence_for_the_same_seed_when_run_twice() {
    let mut first = Rng::seeded(0xF00D);
    let mut second = Rng::seeded(0xF00D);
    for _ in 0..1000 {
        assert_eq!(first.next_u64(), second.next_u64());
    }
}

#[test]
fn should_produce_a_different_sequence_for_a_different_seed_when_compared() {
    let mut first = Rng::seeded(1);
    let mut second = Rng::seeded(2);
    let differences = (0..100)
        .filter(|_| first.next_u64() != second.next_u64())
        .count();
    assert!(
        differences > 90,
        "two seeds must not agree, got {differences}/100"
    );
}

#[test]
fn should_stay_inside_the_requested_range_when_asked_for_a_bounded_value() {
    let mut rng = Rng::seeded(7);
    for _ in 0..10_000 {
        let value = rng.below(13);
        assert!(value < 13, "{value} escaped the bound");
    }
    assert_eq!(
        rng.below(0),
        0,
        "an empty range yields zero rather than panicking"
    );
}

#[test]
fn should_pick_from_a_slice_when_one_is_offered() {
    let mut rng = Rng::seeded(3);
    let alphabet = ["a", "b", "c"];
    let mut seen = [false; 3];
    for _ in 0..200 {
        let picked = rng
            .pick(&alphabet)
            .expect("a non-empty slice always yields");
        let index = alphabet
            .iter()
            .position(|c| c == picked)
            .expect("from the slice");
        seen[index] = true;
    }
    assert!(seen.iter().all(|s| *s), "every element must be reachable");
    assert_eq!(rng.pick::<u8>(&[]), None);
}

#[test]
fn should_build_a_string_from_a_token_alphabet_when_generating_a_case() {
    let mut rng = Rng::seeded(11);
    let alphabet = ["get", " ", "|", "where", "\"", "$", "42"];
    for _ in 0..500 {
        let generated = rng.assemble(&alphabet, 12);
        assert!(generated.len() <= 12 * 5);
        // Whatever comes out must be something a decoder can be handed: valid UTF-8, and made
        // only of the pieces it was given.
        let mut rest = generated.as_str();
        while !rest.is_empty() {
            let matched = alphabet
                .iter()
                .find(|token| rest.starts_with(**token))
                .unwrap_or_else(|| panic!("{generated:?} contains something outside the alphabet"));
            rest = &rest[matched.len()..];
        }
    }
}

#[test]
fn should_generate_the_same_corpus_for_the_same_seed_when_a_failure_is_replayed() {
    // The point of a seeded fuzzer: a failing case can be reproduced from the seed printed in
    // the failure message, rather than being gone by the time anyone looks.
    let corpus = |seed: u64| {
        let mut rng = Rng::seeded(seed);
        (0..50)
            .map(|_| rng.assemble(&["a", "|", " "], 8))
            .collect::<Vec<_>>()
    };
    assert_eq!(corpus(99), corpus(99));
}
