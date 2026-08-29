//! The mutator: how one input becomes the next.
//!
//! There is no coverage feedback here (ADR-0313), so the mutator carries the whole burden of
//! reaching past a decoder's first rejection. It works the way a coverage-guided mutator works
//! at the byte level — flip, splice, insert, delete, repeat — with two additions aimed at the
//! decoders spec §35.6 names: the *interesting* byte and word values that sit on the edges of a
//! length field, and a repeat operator large enough to build the nesting and length bombs those
//! decoders are supposed to refuse.

use ono_testkit::Rng;

/// Byte values that sit on a boundary a decoder is likely to have got wrong.
const INTERESTING_BYTES: &[u8] = &[0x00, 0x01, 0x7f, 0x80, 0xff, b'"', b'\\', b'\n', b'{', b'['];

/// 32-bit values that sit on a boundary a length field is likely to have got wrong.
const INTERESTING_WORDS: &[u32] = &[
    0,
    1,
    u32::MAX,
    u32::MAX - 1,
    0x8000_0000,
    0x0000_0fff,
    0x0010_0000,
    0x0010_0001,
];

/// The upper bound on an input the mutator will produce.
///
/// A bomb has to be big enough to be a bomb and small enough that a bounded run stays bounded:
/// the repeat operator below multiplies, and a corpus entry that is itself tens of kilobytes
/// would otherwise grow without limit across a campaign. 64 kB is far past every decoder's own
/// ceiling and still executes in milliseconds.
const MAX_INPUT: usize = 1 << 16;

/// Turns one input into the next, deterministically for a given seed.
#[derive(Debug)]
pub struct Mutator {
    rng: Rng,
}

impl Mutator {
    /// A mutator whose whole sequence is fixed by `seed`.
    #[must_use]
    pub fn new(seed: u64) -> Self {
        Self {
            rng: Rng::seeded(seed),
        }
    }

    /// The next input, built from `base` and — where the operator splices — from `other`.
    #[must_use]
    pub fn mutate(&mut self, base: &[u8], other: &[u8]) -> Vec<u8> {
        let mut out = base.to_vec();
        // Several operators per input: one bit flip rarely gets past a length check, and the
        // interesting inputs are the ones that are wrong in two places at once.
        let rounds = 1 + self.rng.below(4);
        for _ in 0..rounds {
            self.apply(&mut out, other);
        }
        out.truncate(MAX_INPUT);
        out
    }

    fn apply(&mut self, out: &mut Vec<u8>, other: &[u8]) {
        match self.rng.below(10) {
            0 => self.flip_bit(out),
            1 => self.set_interesting_byte(out),
            2 => self.set_interesting_word(out),
            3 => self.insert(out),
            4 => self.delete(out),
            5 => self.duplicate(out),
            6 => self.splice(out, other),
            7 => self.truncate(out),
            8 => self.repeat(out),
            _ => self.swap(out),
        }
    }

    fn index(&mut self, out: &[u8]) -> Option<usize> {
        (!out.is_empty()).then(|| self.rng.below(out.len()))
    }

    fn flip_bit(&mut self, out: &mut [u8]) {
        if let Some(at) = self.index(out) {
            let bit = self.rng.below(8);
            out[at] ^= 1 << bit;
        }
    }

    fn set_interesting_byte(&mut self, out: &mut [u8]) {
        if let Some(at) = self.index(out)
            && let Some(value) = self.rng.pick(INTERESTING_BYTES)
        {
            out[at] = *value;
        }
    }

    fn set_interesting_word(&mut self, out: &mut [u8]) {
        if out.len() < 4 {
            return;
        }
        let at = self.rng.below(out.len() - 3);
        let Some(value) = self.rng.pick(INTERESTING_WORDS).copied() else {
            return;
        };
        let bytes = if self.rng.chance(2) {
            value.to_be_bytes()
        } else {
            value.to_le_bytes()
        };
        out[at..at + 4].copy_from_slice(&bytes);
    }

    fn insert(&mut self, out: &mut Vec<u8>) {
        let at = self.rng.below(out.len() + 1);
        let count = 1 + self.rng.below(16);
        let filler = self.rng.pick(INTERESTING_BYTES).copied().unwrap_or(0);
        for offset in 0..count {
            out.insert(at + offset, filler);
        }
    }

    fn delete(&mut self, out: &mut Vec<u8>) {
        let Some(at) = self.index(out) else { return };
        let count = 1 + self.rng.below(out.len() - at);
        out.drain(at..at + count);
    }

    fn duplicate(&mut self, out: &mut Vec<u8>) {
        let Some(at) = self.index(out) else { return };
        let count = 1 + self.rng.below(out.len() - at);
        let chunk = out[at..at + count].to_vec();
        let to = self.rng.below(out.len() + 1);
        out.splice(to..to, chunk);
    }

    fn splice(&mut self, out: &mut Vec<u8>, other: &[u8]) {
        if other.is_empty() {
            return;
        }
        let from = self.rng.below(other.len());
        let count = 1 + self.rng.below(other.len() - from);
        let at = self.rng.below(out.len() + 1);
        out.splice(at..at, other[from..from + count].iter().copied());
    }

    fn truncate(&mut self, out: &mut Vec<u8>) {
        if let Some(at) = self.index(out) {
            out.truncate(at);
        }
    }

    /// Repeats a small chunk many times: how a nesting bomb or a length bomb is built.
    fn repeat(&mut self, out: &mut Vec<u8>) {
        let Some(at) = self.index(out) else { return };
        let count = 1 + self.rng.below(4.min(out.len() - at));
        let chunk = out[at..at + count].to_vec();
        let times = [8, 64, 1_024, 16_384][self.rng.below(4)];
        let mut bomb = Vec::with_capacity(chunk.len() * times);
        for _ in 0..times {
            bomb.extend_from_slice(&chunk);
        }
        let to = self.rng.below(out.len() + 1);
        out.splice(to..to, bomb);
    }

    fn swap(&mut self, out: &mut [u8]) {
        if out.len() < 2 {
            return;
        }
        let a = self.rng.below(out.len());
        let b = self.rng.below(out.len());
        out.swap(a, b);
    }
}
