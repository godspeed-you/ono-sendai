//! The child-process end of the subprocess-transport suite.
//!
//! This binary is what `tests/subprocess.rs` spawns instead of `ssh <host> ono --agent`: the
//! same agent loop the real `ono --agent` will run, over stdin/stdout, serving the deterministic
//! fixture registry. It exists so the SSH fallback of spec §37 Phase H is provable offline —
//! the transport cannot tell a local child from a remote shell, which is the property under
//! test. It is not part of the product.

// The same fixture the in-process suites use, so the child serves exactly the same objects.
#[path = "../../tests/common/fixture.rs"]
mod fixture;

use std::process::ExitCode;
use std::sync::Arc;

use ono_protocol::Identity;
use ono_remote::{AgentConfig, agent_main};

fn main() -> ExitCode {
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("ono-remote-fixture-agent: cannot start a runtime: {error}");
            return ExitCode::FAILURE;
        }
    };
    let registry = fixture::fixture_registry(Arc::new(fixture::FixtureObserved::default()));
    let config = AgentConfig::new(registry).with_identity(Identity::new("remote-user"));
    runtime.block_on(agent_main(tokio::io::stdin(), tokio::io::stdout(), config))
}
