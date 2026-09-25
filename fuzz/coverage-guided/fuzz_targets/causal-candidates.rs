#![no_main]
//! The `causal-candidates` target of v0.5 §47.3, driven by libFuzzer.
//!
//! The body is `ono_fuzz`'s, so the deterministic tier the gate runs and this one execute the
//! same code on the same corpus. A finding here reproduces with
//! `cargo run -p ono-fuzz -- repro causal-candidates <file>` on the pinned stable toolchain (ADR-0521).

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    let target =
        ono_fuzz::target("causal-candidates").expect("`causal-candidates` is a declared target");
    (target.run)(data);
});
