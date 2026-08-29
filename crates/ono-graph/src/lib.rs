//! The relationship graph and `trace` of the Ono-Sendai shell (spec §22).
//!
//! `trace` is the feature spec §22 opens by calling "the most cyberpunk-looking major feature",
//! immediately followed by the condition that keeps it honest: "it must be grounded in real
//! provider data". This crate is that condition, written down.
//!
//! # What lives here
//!
//! - [`Graph`], [`Node`], [`Edge`] — the graph value of spec §22.1. A graph is an ordinary
//!   [`Value`](ono_value::Value): it travels a pipeline, `to json` serializes it, and nothing
//!   about it is a drawing.
//! - [`RelationshipProvider`] — how edges are contributed, shaped like
//!   [`ono_provider_api::Provider`] so that the KUANG/11 relationship contribution of spec §31.26
//!   is not a special case of anything.
//! - [`ProcessTree`], [`OpenFiles`], [`ProcessSockets`], [`SocketOwners`], [`ServiceProcesses`],
//!   [`ServiceDependencies`],
//!   [`FileHolders`], [`MountDevices`], [`MountFilesystems`], [`MountPeers`], [`MountUsers`],
//!   [`RouteInterfaces`], [`InterfaceRoutes`], [`InterfaceSockets`], [`UserProcesses`],
//!   [`UserGroups`], [`ProcessUsers`] — the exact relationships of spec §22.2 and §22.3, each
//!   read from the kernel or the account database; [`HostLinks`] and [`LinkProviders`] — the
//!   links of spec §21 and what they negotiated, read from the shell's own tables.
//!   [`ContainerImage`] — the container-to-image relationship of spec §9.1, read from the
//!   container engine.
//! - [`RemoteHosts`] with [`Resolver`] — the derived one, marked as derived.
//! - [`Tracer`] with [`TraceOptions`] — the bounded walk of spec §22.3.
//! - [`Graph::trees`] — the ASCII shape of spec §22.4, through `ono-render`'s tree renderer.
//!
//! # Exact and inferred are not the same thing
//!
//! Spec §22.2: "Ono-Sendai MUST distinguish exact relationships from inferred ones… The UI must
//! not visually imply certainty that the provider does not possess."
//!
//! Every [`Edge`] carries a [`Confidence`], set by the provider that produced it. There is no
//! way to change it afterwards: [`Edge::exact`] and [`Edge::inferred`] are the only constructors,
//! nothing else writes the field, and an inference must name its evidence at the moment it is
//! made (spec §31.25). A drawing follows suit — an observation is `+--`, an inference is `+~~`,
//! in the text itself rather than in a colour a pipe would strip.
//!
//! # Building a trace
//!
//! ```no_run
//! use std::sync::Arc;
//! use ono_graph::{TraceOptions, Tracer, kernel_relationships, roots};
//! use ono_provider_api::{ProviderRegistry, Query, Selector};
//! use ono_render::Layout;
//! use ono_value::Value;
//!
//! # async fn example(registry: Arc<ProviderRegistry>) -> Result<(), ono_value::ErrorValue> {
//! // `trace process 812`
//! let query = Query::target("process").with(Selector::field("pid", Value::Int(812)));
//! let tracer = Tracer::new()
//!     .with_all(kernel_relationships(Arc::clone(&registry)))
//!     .with_options(TraceOptions::from_query(&query));
//!
//! let graph = tracer.trace(roots(&registry, &query).await?).await;
//!
//! // As data, for a pipeline:
//! let value = graph.to_value()?;
//! // Or as the drawing of spec §22.4:
//! for tree in graph.trees() {
//!     for line in Layout::new(100).render_tree(&tree) {
//!         println!("{line}");
//!     }
//! }
//! # Ok(())
//! # }
//! ```

#![forbid(unsafe_code)]

mod graph;
mod kernel;
mod label;
mod provider;
mod trace;
mod tree;

pub use graph::{
    Direction, Edge, Graph, GraphFailure, Node, TRACE_PROVIDER, Truncation, confidence_name,
};
pub use kernel::{
    ContainerImage, FileHolders, HostLinks, InterfaceRoutes, InterfaceSockets, LinkProviders,
    MountDevices, MountFilesystems, MountPeers, MountUsers, OpenFiles, ProcessSockets, ProcessTree,
    ProcessUsers, RemoteHosts, Resolver, RouteInterfaces, ServiceDependencies, ServiceProcesses,
    SocketOwners, UserGroups, UserProcesses, kernel_relationships, rooted_relationships,
};
pub use label::label_of;
pub use provider::{Relationship, RelationshipProvider, Relationships};
pub use trace::{DEFAULT_DEPTH, DEFAULT_MAX_NODES, TraceOptions, Tracer, roots};

/// Whether a provider observed a relationship or derived it (spec §22.2).
///
/// Re-exported from `ono-render`, where the tree renderer already draws the distinction: one
/// definition of confidence, so a value and its drawing cannot disagree about it.
pub use ono_render::Confidence;
