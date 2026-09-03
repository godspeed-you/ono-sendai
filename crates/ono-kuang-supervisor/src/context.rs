//! What `context.get` answers with: the shell's context stack, published by the host
//! (spec §31.12's `context` domain, ADR-0567).
//!
//! The supervisor has no session of its own. The shell publishes its context — where the
//! session stands, what it entered, which link it is on — through a source the host hands the
//! loader, and the test host hands a fixed one so a conformance run is deterministic
//! (spec §31.73).

use serde_json::{Value as Json, json};

/// Where the context of spec §31.12 comes from.
pub trait ContextSource: Send + Sync + std::fmt::Debug {
    /// `{cwd: path, object: value?, link: string?, host: string, interactive: bool,
    /// redirected: bool}` — the context stack, and nothing beyond it.
    fn context(&self) -> Json;
}

/// A context that never changes: the test host's, and the loader's default.
#[derive(Debug, Clone)]
pub struct FixedContext(pub Json);

impl FixedContext {
    /// The deterministic context of spec §31.73: a session at `/`, in nothing, on no link.
    #[must_use]
    pub fn test_host() -> Self {
        Self(json!({
            "cwd": "/",
            "object": null,
            "link": null,
            "host": "test-host",
            "interactive": false,
            "redirected": true,
        }))
    }
}

impl ContextSource for FixedContext {
    fn context(&self) -> Json {
        self.0.clone()
    }
}
