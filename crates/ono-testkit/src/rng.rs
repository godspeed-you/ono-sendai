//! A small, reproducible pseudo-random generator for fuzz-style tests.
//!
//! Spec §35.6 requires fuzzing of every parser and decoder; AGENTS.md §11 requires every test to
//! be deterministic. Both hold only if the randomness is reproducible, so this is a fixed
//! algorithm seeded explicitly by each test rather than a dependency whose sequence may change
//! between versions. A failing case is reproduced from the seed in the failure message.

/// A `xoshiro256**` generator: fast, tiny, and identical on every machine and every run.
#[derive(Debug, Clone)]
pub struct Rng {
    state: [u64; 4],
}

impl Rng {
    /// A generator seeded with `seed`.
    ///
    /// ```
    /// use ono_testkit::Rng;
    /// let mut a = Rng::seeded(42);
    /// let mut b = Rng::seeded(42);
    /// assert_eq!(a.next_u64(), b.next_u64());
    /// ```
    #[must_use]
    pub fn seeded(seed: u64) -> Self {
        // SplitMix64 expands one seed into four words, so that neighbouring seeds do not produce
        // correlated streams.
        let mut z = seed;
        let mut next = || {
            z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut value = z;
            value = (value ^ (value >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            value = (value ^ (value >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            value ^ (value >> 31)
        };
        Self {
            state: [next(), next(), next(), next()],
        }
    }

    /// The next value in the sequence.
    pub fn next_u64(&mut self) -> u64 {
        let result = self.state[1].wrapping_mul(5).rotate_left(7).wrapping_mul(9);
        let t = self.state[1] << 17;

        self.state[2] ^= self.state[0];
        self.state[3] ^= self.state[1];
        self.state[1] ^= self.state[2];
        self.state[0] ^= self.state[3];
        self.state[2] ^= t;
        self.state[3] = self.state[3].rotate_left(45);

        result
    }

    /// A value below `bound`, or zero when `bound` is zero.
    pub fn below(&mut self, bound: usize) -> usize {
        if bound == 0 {
            return 0;
        }
        // Modulo bias is irrelevant for generating test inputs and costs a branch to avoid.
        usize::try_from(self.next_u64() % bound as u64).unwrap_or(0)
    }

    /// An element of `slice`, or `None` when it is empty.
    pub fn pick<'a, T>(&mut self, slice: &'a [T]) -> Option<&'a T> {
        if slice.is_empty() {
            return None;
        }
        let index = self.below(slice.len());
        slice.get(index)
    }

    /// A string of at most `pieces` elements drawn from `alphabet`.
    ///
    /// Building inputs from a token alphabet rather than from random bytes is what makes a
    /// fuzz-style test reach past a decoder's first rejection: random bytes almost never look
    /// like a command line, while `["get", " ", "|", "\""]` almost always does.
    pub fn assemble(&mut self, alphabet: &[&str], pieces: usize) -> String {
        let count = self.below(pieces + 1);
        let mut assembled = String::new();
        for _ in 0..count {
            if let Some(piece) = self.pick(alphabet) {
                assembled.push_str(piece);
            }
        }
        assembled
    }

    /// `true` with probability one in `odds`.
    pub fn chance(&mut self, odds: usize) -> bool {
        odds != 0 && self.below(odds) == 0
    }
}
