#![no_main]
//! The `recovery-metadata` target of v0.6 §54.3, driven by libFuzzer.
//!
//! The body is `ono_fuzz`'s, so the deterministic tier the gate runs and this one execute the
//! same code on the same corpus. A finding here reproduces with
//! `cargo run -p ono-fuzz -- repro recovery-metadata <file>` on the pinned stable toolchain (ADR-0521).

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    let target =
        ono_fuzz::target("recovery-metadata").expect("`recovery-metadata` is a declared target");
    (target.run)(data);
});
