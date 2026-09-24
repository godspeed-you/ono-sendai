# ADR-0866: Every shipped binary is held to the budget of its triple where it is packaged

- Status: accepted
- Date: 2026-09-24
- Spec refs: v0.4.1 §44.2, §46.5, §50.1; ADR-0121, ADR-0123, ADR-0863, ADR-0864, ADR-0865,
  ADR-0870, ADR-0905, ADR-0912
- Issues: #125
- Decided by: agent (autonomous)

## Context

ADR-0864 enforced the size budget in the acceptance image, for the x86_64 `ono` only. An
independent review found that the budget never reached the bytes a release ships:

- `.github/workflows/release.yml` builds the packages with `scripts/package.sh` on a native
  x86_64 and a native arm64 runner, and nothing there measured them. The aarch64 `ono` was never
  measured, recorded or budgeted, although #125 asks for the size "per target triple".
- `kuang-compile` ships in the same packages since ADR-0870, and ADR-0864 §3 recorded it without
  a budget.
- The runners that build what ships have no `xtask`; `scripts/build-core.sh` had solved the same
  problem for the core budget with an `awk` of its own.

## Decision

1. **`scripts/binary-size.sh --triple <triple> <binary>...` holds binaries to their budgets
   where no `xtask` is.** It reads `build_budgets` from `docs/contracts/hardening/limits.yaml`
   with the rule `cargo xtask binary-size` applies — the row naming the binary and the triple,
   else the binary's row naming no triple — prints one `binary-size:` line per binary, and fails
   when a binary is over its budget **or when no row applies to it on that triple**: a shipped
   triple nobody budgeted is a failure, not a pass. `xtask/tests/metrics.rs` holds the script and
   `xtask` to the same answer for every row of the registry, at the budget and one byte over.
2. **`scripts/package.sh` runs it on every binary it packages**, after the build and before any
   package is written, for the triple it built — the native path and the `cross` path alike, and
   with `--no-build`. So the release workflow's `package` and `rebuild` jobs, on both
   architectures, and CI's `installable packages` job on every push, refuse a binary over its
   budget, and their logs state the size of every binary that ships.
3. **`scripts/build-core.sh` uses the same script** instead of its own `awk` for
   `build.ono_core_stripped_bytes`; one reader of the registry for every script that builds.
4. **Every row names its triple, and every shipped binary has one:**

   | key | binary | triple | budget | measured at cc2d9a5d + this fix |
   |---|---|---|---|---|
   | `build.ono_stripped_bytes` | `ono` | x86_64-unknown-linux-gnu | 24 350 304 (ADR-0864) | 22 134 752 |
   | `build.ono_aarch64_stripped_bytes` | `ono` | aarch64-unknown-linux-gnu | 21 359 924 | 19 418 112 |
   | `build.ono_core_stripped_bytes` | `ono` | x86_64-unknown-linux-musl | 7 999 999 (ADR-0912) | 7 063 744 |
   | `build.kuang_compile_stripped_bytes` | `kuang-compile` | x86_64-unknown-linux-gnu | 8 045 823 | 7 314 384 |
   | `build.kuang_compile_aarch64_stripped_bytes` | `kuang-compile` | aarch64-unknown-linux-gnu | 6 151 174 | 5 591 976 |

   The new budgets are the measurement plus ten percent, rounded up, the rule ADR-0863 set for
   the first one. `kuang-compile` is budgeted now, reversing ADR-0864 §3: it ships in every
   package, and a budget the packaging step enforces costs nothing once the step exists.
5. **One caller packages unmeasured, out loud.** `scripts/rebuild-check.sh --binary <path>`
   compares two packaging runs of a binary it was handed — the gate's packaging suite hands it
   its own test executable — and that binary's size is not the size of anything that ships. It
   passes `package.sh --size-unmeasured <why>`, which prints `binary-size: not measured — <why>`
   on the line the check would have printed, and skips the check. Without `--binary`,
   `rebuild-check.sh` packages the build it finds and the check runs.
6. **The aarch64 figures come from `cross`.** They were measured with `scripts/package.sh
   --target aarch64-unknown-linux-gnu` on x86_64, which builds in cross's toolchain image; the
   release builds natively on arm64 in the pinned `rust` image. The compiler, its version, the
   profile and the dependency graph are the same; the linker and the C runtime objects differ,
   which moved the x86_64 figure by 0.1 % between lld and GNU ld (ADR-0865). Ten percent of
   headroom is two orders of magnitude above that. The first native release run prints the native
   figure in its log.

## Correction to ADR-0865

ADR-0865 §3 says CI uploads `target/cargo-timings/cargo-timing.html`. Since ADR-0905 the
packaging build is two cargo invocations, the second overwrites that file, and `ci.yml` uploads
`target/cargo-timings/cargo-timing-*.html` — one timestamped report per invocation. The
artifact is the one ADR-0865 describes; its file names are these.

## Consequences

- A release whose `ono` or `kuang-compile` outgrew its budget on either architecture fails in the
  job that built it, before a package exists, and says by how much.
- A new triple — a riscv64 package, say — fails its first packaging run until a row budgets it.
- Two readers of one registry exist, in Rust and in `awk`; the agreement test is what keeps them
  one rule.
- Tests: `xtask/tests/harness.rs` —
  `should_package_every_shipped_binary_within_its_budget_and_say_how_large_it_is`,
  `should_refuse_to_package_a_binary_over_the_budget_of_its_triple`,
  `should_refuse_to_package_for_a_triple_no_budget_covers`,
  `should_say_so_when_it_packages_a_binary_it_does_not_measure` (the real `package.sh
  --no-build` with stand-in packagers and binaries); `xtask/tests/metrics.rs` —
  `should_hold_a_shipped_binary_to_the_budget_of_its_triple_without_xtask`,
  `should_read_every_budget_of_this_repository_as_xtask_reads_it`.

## Alternatives considered

- **Check in `scripts/package-check.sh`.** It sees the packages rather than the build, which
  would mean unpacking them; and it runs after the packages exist. `package.sh` holds the binaries
  before it packages them, on the same runner.
- **Carry `xtask` onto the release runners.** Compiling it there compiles the workspace a second
  time in debug; ADR-0864 rejected it for the same reason.
- **One triple-less row per binary.** It would have covered aarch64 with the x86_64 figure, which
  is 14 % larger, and it would cover the next triple silently.
