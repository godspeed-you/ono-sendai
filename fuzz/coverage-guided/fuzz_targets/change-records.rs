#![no_main]
//! The `change-records` target of v0.6 §54.3, driven by libFuzzer.
//!
//! The body is `ono_fuzz`'s, so the deterministic tier the gate runs and this one execute the
//! same code on the same corpus. A finding here reproduces with
//! `cargo run -p ono-fuzz -- repro change-records <file>` on the pinned stable toolchain (ADR-0521).

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    let target = ono_fuzz::target("change-records").expect("`change-records` is a declared target");
    (target.run)(data);
});
