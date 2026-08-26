//! Entry and session identities.
//!
//! Spec §20.1 gives the entry an id so a structured result, a note or a remote session can refer
//! to it. The identity is a UUIDv7: time-ordered, so sorting by id sorts by when, and unique
//! without a coordinator, so a remote link's entries and a local session's entries can share one
//! namespace (phase H) without renumbering.

use std::sync::atomic::{AtomicU64, Ordering};

/// Generates time-ordered, unique identities for one shell session.
#[derive(Debug)]
pub(crate) struct IdSource {
    session: String,
    entropy: AtomicU64,
    counter: AtomicU64,
}

impl IdSource {
    /// A source seeded from the wall clock and the process id.
    ///
    /// The seed does not need to be unpredictable — an entry id is not a secret and is never a
    /// capability — only unlikely to collide with another shell started in the same millisecond.
    pub(crate) fn new() -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos() as u64);
        let seed = mix(now ^ (u64::from(std::process::id()) << 32));
        let source = Self {
            session: String::new(),
            entropy: AtomicU64::new(seed),
            counter: AtomicU64::new(0),
        };
        let session = source.next_id();
        Self {
            session,
            entropy: AtomicU64::new(seed),
            counter: AtomicU64::new(0),
        }
    }

    /// The identity of this shell session.
    pub(crate) fn session(&self) -> &str {
        &self.session
    }

    /// The next entry identity, in canonical UUID form.
    pub(crate) fn next_id(&self) -> String {
        let millis = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_millis() as u64)
            & 0x0000_FFFF_FFFF_FFFF;
        let sequence = self.counter.fetch_add(1, Ordering::Relaxed);
        let previous = self.entropy.load(Ordering::Relaxed);
        let entropy = mix(previous ^ sequence.wrapping_mul(0x9E37_79B9_7F4A_7C15));
        self.entropy.store(entropy, Ordering::Relaxed);

        // UUIDv7 layout: 48 bits of milliseconds, version 7, 12 bits ordering within the
        // millisecond, variant, then 62 bits that only have to be distinct.
        let time_low = (millis >> 16) as u32;
        let time_mid = (millis & 0xFFFF) as u16;
        let version_and_seq = 0x7000 | ((sequence & 0x0FFF) as u16);
        let variant_and_rand = 0x8000 | ((entropy >> 48) as u16 & 0x3FFF);
        let tail = entropy & 0x0000_FFFF_FFFF_FFFF;

        format!(
            "{time_low:08x}-{time_mid:04x}-{version_and_seq:04x}-{variant_and_rand:04x}-{tail:012x}"
        )
    }
}

/// SplitMix64's finaliser: cheap, and good enough to keep neighbouring seeds apart.
fn mix(value: u64) -> u64 {
    let mut z = value.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

impl Default for IdSource {
    fn default() -> Self {
        Self::new()
    }
}
