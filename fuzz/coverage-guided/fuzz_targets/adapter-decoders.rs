#![no_main]
//! The `adapter-decoders` target of v0.4.1 §41.2, driven by libFuzzer.
//!
//! The body is `ono_fuzz`'s, so the deterministic tier the gate runs and this one execute the
//! same code on the same corpus. A finding here reproduces with
//! `cargo run -p ono-fuzz -- repro adapter-decoders <file>` on the pinned stable toolchain (ADR-0521).

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    let target = ono_fuzz::target("adapter-decoders").expect("`adapter-decoders` is a declared target");
    (target.run)(data);
});
