//! The fuzz targets of spec §35.6.
//!
//! > Fuzz parser, serializers, remote protocol, plugin protocol and procfs/netlink decoders. A
//! > shell consumes adversarial filenames and external output by nature. — spec §35.6
//!
//! Five targets, one per area, each a function from arbitrary bytes to nothing whose only
//! contract is that it returns without panicking. Around them sits a small mutation engine —
//! seeds from `corpus/`, deterministic mutations, panics caught and written to `artifacts/` —
//! so a bounded run fits inside the quality gate and a finding reproduces from a committed file.
//!
//! What this is not: coverage-guided. The pinned stable toolchain has no `libFuzzer`, and a
//! coverage-instrumented build is not something the gate can run in seconds. ADR-0313 records
//! that limit, what it costs and what would lift it. What is here finds panics and pathological
//! slowness, from a corpus built out of the shapes the decoders actually meet.
//!
//! ```no_run
//! let target = ono_fuzz::target("parser").expect("the parser target");
//! let corpus = ono_fuzz::load_for(target.name);
//! let report = ono_fuzz::run(target, &corpus, &ono_fuzz::Budget::default());
//! assert!(report.findings.is_empty());
//! ```

mod corpus;
mod engine;
mod mutate;
mod targets;

pub use corpus::{artifacts_dir, corpus_dir, digest, load, load_for, record, root};
pub use engine::{Budget, Fault, Finding, Report, run};
pub use mutate::Mutator;
pub use targets::{TARGETS, Target, target};
