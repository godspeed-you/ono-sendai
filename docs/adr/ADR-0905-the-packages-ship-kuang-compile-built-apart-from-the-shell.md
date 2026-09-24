# ADR-0905: The packages ship `kuang-compile`, built apart from the shell

- Status: accepted
- Date: 2026-09-24
- Spec refs: v0.4.1 §48.1, §48.2; spec §31.10, §31.36; ADR-0121, ADR-0122, ADR-0870, ADR-0902
- Decided by: agent (autonomous)

## Context

ADR-0870 took Cranelift out of `ono`. `install plugin` now runs the SDK's `kuang-compile` on a
component, and looks for it beside `ono` first and on `PATH` second. ADR-0870 left one point open
under "Hard, and open": **the distribution packages ship `ono` only.** A shell installed from our
`.deb` or `.rpm` therefore refuses to install every component package. Before ADR-0870 it
installed them, so this is a regression of released behaviour.

## Decision

**Both packages install `/usr/bin/kuang-compile` (root:root 0755) beside `/usr/bin/ono`. The
release builds it in a cargo invocation of its own, and package validation checks that the
packaged `ono` contains no Cranelift code generator.**

- `crates/ono-cli/Cargo.toml` lists `target/release/kuang-compile` as an asset in both
  `[package.metadata.deb]` and `[package.metadata.generate-rpm]`. The deb's `$auto` computes
  its library dependencies the same way it does for `ono`.
- `scripts/package.sh` runs `cargo build --release --locked --target <t> --package ono-cli`, then
  `… --package ono-kuang-sdk --bin kuang-compile`, as two invocations; `cross` gets the same pair.
  Cargo unifies features within one invocation, and the SDK enables the supervisor's `compiler`
  feature, so building the two together would link Cranelift into `ono`. `--no-build` refuses a
  target directory that lacks either binary and names the missing one. The RPM stage copies both
  binaries with their mtimes set to the epoch (ADR-0902).
- `scripts/rebuild-check.sh` packages the `kuang-compile` found beside the `--binary` it is given.
- `scripts/package-check.sh`, for both formats, checks each binary's ELF machine and scans it for
  private build paths, applies the glibc floor to each, and then:
  - fails if the packaged `ono` contains `cranelift_codegen` / `cranelift-codegen`;
  - fails if the packaged `kuang-compile` does *not* contain it, because that would mean the
    probe cannot detect the compiler at all.
- In the fresh containers it also checks that `kuang-compile` is present at 0755 root:root, that
  `kuang-compile --help` works, and that it refuses bytes that are not a component with a reason.
  After removal it checks that the binary is gone.
- `kuang-sign` still does not ship. It is a publisher's tool; installing a package does not run
  it, so no released behaviour depends on it.

This closes ADR-0870's first open point. ADR-0906 closes its second, the notices.

## Consequences

- A shell installed from the packages can install component packages again.
  `xtask/tests/packaging.rs::should_build_a_deb_…`, `::should_build_an_rpm_…` and
  `::should_normalize_file_ownership_…` now require the second binary.
  `::should_refuse_to_package_a_shell_without_the_compiler_it_installs_components_with` and
  `::should_build_the_compiler_apart_from_the_shell_and_prove_the_shell_went_without_it` cover
  the refusal and the separate build.
- The packages grow by the size of `kuang-compile`, which carries wasmtime's compiler. That
  compiler now ships in the package, as ADR-0870 anticipated, but not inside `ono`.
- Package validation does not install and load a real component: the containers have no network
  and `dist/` holds no component. Case 214 and the SDK's `compiled.rs` tests prove that path
  against the image's `kuang-compile`. Here the packaged tool is only shown to run and to refuse
  input that is not a component.

## Alternatives considered

- **Look up `kuang-compile` only on `PATH` and ship a separate package for it.** A second package
  means a second name, a second validation, and a dependency between the two, just to relocate
  one binary that every component install needs.
- **Build both binaries in one invocation and trust the feature split.** That fails silently,
  which is exactly what the separate invocations and the probe exist to prevent.
