//! The v0.6 §54.3 surfaces the first three change targets do not reach.
//!
//! > Fuzz: plan parser/block grammar; plan/asset deserialization; provider protocol messages;
//! > snapshot names and paths; impact graph inputs; recovery metadata. — v0.6 §54.3
//!
//! `change-records` already reads the plan and asset codecs, and `storage-tool-output` the output
//! of the storage tools. What is here is the rest of the list: the plan block and the references
//! an operator types, the contribution documents and recovery messages a KUANG/11 provider sends,
//! the snapshot names and subvolume paths a provider derives, the graph a store hands back and
//! the world impact is derived over, and the recovery metadata a store and a file archive keep.
//!
//! As everywhere in this crate, each body must return for every input, and each checks the
//! invariant its decoder promises beyond "did not panic".

use std::sync::Arc;

use ono_change_core::value::{
    impact_from_record, impact_record, recovery_plan_from_record, recovery_plan_record,
};
use ono_change_core::{
    ChangePlan, FrozenTarget, ImpactClass, ImpactGraph, ImpactNode, Intent, PlanId,
    RecoveryAssetId, UnknownBoundary,
};
use ono_change_plan::builder::{BlockPlan, BlockStatement, MAX_BLOCK_ACTIONS};
use ono_change_plan::references::{Reference, ReferenceKind};
use ono_kuang_protocol::{
    ChangeViewDocument, ImpactProviderDocument, PlanContributeParams, RecoveryCandidateWire,
    RecoveryCleanupParams, RecoveryDiscoverParams, RecoveryEstimateCostParams,
    RecoveryPrepareParams, RecoveryProviderDocument, RecoveryQuiesceParams, RecoveryRestoreParams,
    RecoveryValidateParams, RiskRuleDocument, VerificationObserveParams,
    VerificationProviderDocument,
};

// --- plan parser/block grammar -----------------------------------------------------------------

/// The plan block of §5.2 and the references of §36.4, over whatever the operator typed.
///
/// §5.2's contract is that a block is a bounded list of action descriptions and nothing else, so
/// an accepted block never holds a loop, a function, a background job, control flow or a
/// statement with no verb. §36.4's is that a reference always names an identity. The block text
/// also goes through the shell parser inside its `plan { … }` wrapper, which is how it arrives.
pub(crate) fn plan_grammar(data: &[u8]) {
    let text = String::from_utf8_lossy(data);
    plan_references(&text);
    plan_block(&text);
    let wrapped = format!("plan {{ {text} }}");
    let parsed = ono_parser::parse(&wrapped);
    for diagnostic in parsed.diagnostics() {
        assert!(
            diagnostic.span().end() as usize <= wrapped.len(),
            "a diagnostic span reaches past the end of a plan block"
        );
    }
}

fn plan_references(text: &str) {
    for word in text.split_whitespace().take(64) {
        let Ok(reference) = Reference::parse(word) else {
            continue;
        };
        let names_an_identity = match reference.kind() {
            ReferenceKind::Recovery => RecoveryAssetId::parse(reference.body()).is_some(),
            ReferenceKind::Plan | ReferenceKind::Unqualified => {
                PlanId::parse(reference.body()).is_some()
            }
        };
        assert!(
            names_an_identity,
            "§36.4: `{word}` was accepted as a reference that names no identity"
        );
    }
}

fn plan_block(text: &str) {
    let statements: Vec<BlockStatement> = text
        .split([';', '\n'])
        .filter(|statement| !statement.trim().is_empty())
        .take(MAX_BLOCK_ACTIONS + 8)
        .map(block_statement)
        .collect();
    let count = statements.len();
    let forbidden = statements
        .iter()
        .any(|statement| statement.forbidden_construct().is_some());
    let verbless = statements.iter().any(|statement| {
        matches!(statement, BlockStatement::Action(description)
            if description.verb().trim().is_empty())
    });
    if let Ok(block) = BlockPlan::of(statements) {
        assert!(
            count > 0 && count <= MAX_BLOCK_ACTIONS,
            "§5.2: a block of {count} statements was accepted outside the bound"
        );
        assert!(
            !forbidden,
            "§5.2: a block holding a forbidden construct was accepted"
        );
        assert!(!verbless, "§6.1: a statement with no verb was accepted");
        assert!(
            block.len() == count,
            "§5.2: an accepted block dropped a statement ({} of {count})",
            block.len()
        );
    }
}

/// Classifies one statement the way the shell's `plan { … }` reader does, by its first word.
fn block_statement(statement: &str) -> BlockStatement {
    let trimmed = statement.trim();
    let head = trimmed.split_whitespace().next().unwrap_or_default();
    let source: Arc<str> = Arc::from(trimmed);
    match head {
        "for" | "while" | "loop" => BlockStatement::Loop { source },
        "fn" | "def" => BlockStatement::Function { source },
        "if" | "else" | "match" | "break" | "continue" | "return" | "try" => {
            BlockStatement::ControlFlow { source }
        }
        _ if trimmed.ends_with('&') => BlockStatement::BackgroundJob { source },
        _ => BlockStatement::action(
            head.trim_matches(|character: char| !character.is_alphanumeric()),
            trimmed,
        ),
    }
}

// --- provider protocol messages ----------------------------------------------------------------

/// What a KUANG/11 change or recovery provider sends (§48.2, §12.1).
///
/// With the high bit set, the text is one of the five v0.6 contribution documents a package
/// declares and the host reads before any package code runs. Otherwise it is one of the
/// recovery-provider messages, as JSON; a message that decodes must re-encode to a message that
/// decodes to the same thing, or what the host logs is not what the provider said.
pub(crate) fn provider_messages(data: &[u8]) {
    let Some((selector, rest)) = data.split_first() else {
        return;
    };
    let text = String::from_utf8_lossy(rest);
    if selector & 0x80 != 0 {
        match selector % 5 {
            0 => {
                let _ = RecoveryProviderDocument::parse(&text);
            }
            1 => {
                let _ = ImpactProviderDocument::parse(&text);
            }
            2 => {
                let _ = VerificationProviderDocument::parse(&text);
            }
            3 => {
                let _ = RiskRuleDocument::parse(&text);
            }
            _ => {
                let _ = ChangeViewDocument::parse(&text);
            }
        }
        return;
    }
    macro_rules! round_trip {
        ($message:ty) => {{
            if let Ok(decoded) = serde_json::from_str::<$message>(&text) {
                let encoded = serde_json::to_string(&decoded);
                assert!(
                    encoded.is_ok(),
                    "a decoded provider message does not re-encode"
                );
                if let Ok(encoded) = encoded {
                    let again = serde_json::from_str::<$message>(&encoded);
                    assert!(
                        again.as_ref().is_ok_and(|again| again == &decoded),
                        "a provider message changed across a re-encode: {decoded:?} became \
                         {again:?}"
                    );
                }
            }
        }};
    }
    match selector % 10 {
        0 => round_trip!(RecoveryCandidateWire),
        1 => round_trip!(RecoveryDiscoverParams),
        2 => round_trip!(RecoveryPrepareParams),
        3 => round_trip!(RecoveryValidateParams),
        4 => round_trip!(RecoveryRestoreParams),
        5 => round_trip!(RecoveryCleanupParams),
        6 => round_trip!(RecoveryEstimateCostParams),
        7 => round_trip!(RecoveryQuiesceParams),
        8 => round_trip!(PlanContributeParams),
        _ => round_trip!(VerificationObserveParams),
    }
}

// --- snapshot names and paths ------------------------------------------------------------------

/// The names and paths a storage provider derives from text it did not write (§43.6, Appendix
/// D.8).
///
/// The input is split at the first newline into two strings, which each branch reads as the two
/// things its function takes.
pub(crate) fn snapshot_paths(data: &[u8]) {
    let Some((selector, rest)) = data.split_first() else {
        return;
    };
    let text = String::from_utf8_lossy(rest);
    let (first, second) = text.split_once('\n').unwrap_or((&text, ""));
    match selector % 4 {
        0 => btrfs_names(first, second),
        1 => {
            // Appendix D.8: the recovery namespace must not sit inside a source subvolume, and
            // the test is by path component, so `@snapshots` is not inside `@snap`.
            let config = ono_recovery_btrfs::config::BtrfsConfig::default().snapshots_in(first);
            if config.is_nested_in(second) {
                let location: Vec<&str> = first.trim_matches('/').split('/').collect();
                let source: Vec<&str> = second.trim_matches('/').split('/').collect();
                assert!(
                    location.len() > source.len() && location.starts_with(&source),
                    "Appendix D.8: `{first}` was read as nested in `{second}` without being \
                     beneath it by component"
                );
            }
        }
        2 => {
            // §56.3: a subvolume reference is a whole identity or none; one that reads must read
            // back as the same subvolume.
            if let Some(reference) = ono_recovery_btrfs::SubvolumeRef::parse(&text) {
                let again = ono_recovery_btrfs::SubvolumeRef::parse(&reference.reference());
                assert!(
                    again.as_ref() == Some(&reference),
                    "a subvolume reference read from `{text}` does not read back: {again:?}"
                );
            }
        }
        _ => zfs_names(first, second),
    }
}

fn btrfs_names(plan: &str, subvolume: &str) {
    use ono_recovery_btrfs::config::{sanitised_name, snapshot_name};
    let permitted =
        |character: char| character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.');
    let sanitised = sanitised_name(subvolume);
    assert!(
        !sanitised.is_empty()
            && sanitised.chars().all(permitted)
            && !sanitised.starts_with(['.', '-'])
            && !sanitised.ends_with('.'),
        "§43.6: `{subvolume}` sanitised to `{sanitised}`, which a path or a shell can misread"
    );
    assert!(
        sanitised_name(&sanitised) == sanitised,
        "§43.6: sanitising `{sanitised}` again changed it, so a stored name would drift"
    );
    let name = snapshot_name(plan, subvolume);
    assert!(
        name.starts_with("ono-") && name.chars().all(permitted),
        "Appendix D.8: the snapshot name for `{plan}` and `{subvolume}` is `{name}`"
    );
}

fn zfs_names(dataset: &str, text: &str) {
    use ono_recovery_zfs::naming::{
        full_name, is_permitted, is_valid_snapshot_name, sanitise, snapshot_part,
    };
    let sanitised = sanitise(text);
    assert!(
        sanitised.chars().all(is_permitted)
            && !sanitised.starts_with(['_', '-', '.', ':'])
            && !sanitised.ends_with(['_', '-', '.', ':']),
        "§43.6: `{text}` sanitised to `{sanitised}`"
    );
    assert!(
        sanitise(&sanitised) == sanitised,
        "§43.6: sanitising `{sanitised}` again changed it"
    );
    let part = snapshot_part(Some(text), jiff::Timestamp::UNIX_EPOCH);
    let full = full_name(dataset, &part);
    if is_valid_snapshot_name(&full) {
        assert!(
            full.matches('@').count() == 1 && full.ends_with(part.as_ref()),
            "§43.6: `{full}` was accepted as a snapshot of `{dataset}` it does not name"
        );
    }
    let _ = is_valid_snapshot_name(text);
}

// --- impact graph inputs -----------------------------------------------------------------------

/// The impact graph as a store hands it back, and the world impact is derived over (§9, §36.1).
pub(crate) fn impact_graph(data: &[u8]) {
    let Some((selector, rest)) = data.split_first() else {
        return;
    };
    if selector & 1 == 0 {
        stored_graph(rest);
    } else {
        derived_graph(rest);
    }
}

/// §9.5 derives the blast radius from the nodes, so a graph that survives the store must count
/// the same after it.
fn stored_graph(data: &[u8]) {
    let mut graph = ImpactGraph::empty();
    for chunk in data.chunks(4).take(64) {
        let [id, class, depth, extra] = [0, 1, 2, 3].map(|at| chunk.get(at).copied().unwrap_or(0));
        let class = ImpactClass::ALL
            .get(usize::from(class) % ImpactClass::ALL.len())
            .copied()
            .unwrap_or(ImpactClass::DirectTarget);
        let mut node = ImpactNode::new(
            format!("node-{}", id % 24),
            String::from_utf8_lossy(chunk).into_owned(),
            "ono.file/1",
            class,
            usize::from(depth % 8),
        );
        if extra & 1 == 1 {
            node = node.on_host(format!("host-{}", extra % 5));
        }
        if extra & 2 == 2 {
            node = node.reached_by("process.opened_file");
        }
        graph.add(node);
        if extra & 4 == 4 {
            graph.add_boundary(UnknownBoundary::new(
                format!("node-{}", id % 24),
                "a remote host",
                "no agent is connected",
            ));
        }
    }
    if data.first().is_some_and(|byte| byte & 8 == 8) {
        graph = graph.truncated("the node budget was reached");
    }
    let plan = PlanId::derive(&["ono-fuzz", "impact-graph"]);
    let Ok(record) = impact_record(&plan, &graph) else {
        return;
    };
    let decoded = impact_from_record(&record);
    assert!(
        decoded.as_ref().is_ok_and(|decoded| {
            decoded.nodes().len() == graph.nodes().len()
                && decoded.boundaries().len() == graph.boundaries().len()
                && decoded.blast_radius() == graph.blast_radius()
                && decoded.truncation() == graph.truncation()
        }),
        "§9.5: an impact graph did not survive its own record: {graph:?} became {decoded:?}"
    );
}

/// The walk of §9.3 over a small world whose relations and targets the bytes choose.
///
/// Every frozen target is a direct target (§9.2), no relation node sits deeper than the requested
/// depth, and the walk never collects more relation nodes than the node budget (§9.5).
fn derived_graph(data: &[u8]) {
    use ono_spatial_core::Confidence;
    let Some(world) = impact_world::World::build() else {
        return;
    };
    let mut bytes = data.iter().copied();
    let depth = usize::from(bytes.next().unwrap_or(2) % 5);
    let budget = usize::from(bytes.next().unwrap_or(8) % 12);
    let chosen = bytes.next().unwrap_or(1);
    let mut edges = Vec::new();
    while let (Some(left), Some(right)) = (bytes.next(), bytes.next()) {
        if edges.len() >= 24 {
            break;
        }
        let confidence = if left & 0x80 == 0 {
            Confidence::Exact
        } else {
            Confidence::Inferred
        };
        if let Some(edge) = world.edge(left, right, confidence) {
            edges.push(edge);
        }
    }
    let index = world.index(&edges);
    let mut targets: Vec<FrozenTarget> = world
        .objects()
        .iter()
        .enumerate()
        .filter(|(position, _)| chosen & (1 << (position % 8)) != 0)
        .map(|(_, (object, schema))| impact_world::target_of(object, schema))
        .collect();
    if chosen & 0x80 != 0 {
        targets.push(FrozenTarget::new(
            "ono.package/1",
            "package:openssl",
            "openssl",
        ));
    }
    let request =
        ono_change_impact::ImpactRequest::new(&index, &targets, &[], jiff::Timestamp::UNIX_EPOCH)
            .to_depth(depth)
            .within_nodes(budget);
    let graph = ono_change_impact::derive(&request);
    for target in &targets {
        let id = target.spatial_id().unwrap_or_else(|| target.identity());
        assert!(
            graph
                .nodes()
                .iter()
                .any(|node| node.id() == id && node.class() == ImpactClass::DirectTarget),
            "§9.2: the frozen target {id} is not a direct target of its own impact graph"
        );
    }
    let related: Vec<&ImpactNode> = graph
        .nodes()
        .iter()
        .filter(|node| {
            matches!(
                node.class(),
                ImpactClass::Dependent | ImpactClass::TransitiveRelated
            )
        })
        .collect();
    assert!(
        related.iter().all(|node| node.depth() <= depth),
        "§9.5: the walk went deeper than the {depth} hops it was given"
    );
    assert!(
        related.len() <= budget,
        "§9.5: the walk collected {} relation nodes against a budget of {budget}",
        related.len()
    );
}

mod impact_world {
    use ono_change_core::FrozenTarget;
    use ono_spatial_core::{
        BootIdentity, Confidence, Projection, RelationType, RelationshipEdge, SpatialObject,
        SpatialScope, SpatialType,
    };
    use ono_spatial_index::{FreshnessPolicy, SpatialIndex};
    use ono_value::{Provenance, RecordValue, SchemaId, Value, builtin_schemas};

    const NOW: jiff::Timestamp = jiff::Timestamp::UNIX_EPOCH;

    /// Three files, three processes and two services, as the spatial layer would hold them.
    pub(super) struct World {
        objects: Vec<(SpatialObject, &'static str)>,
    }

    impl World {
        pub(super) fn build() -> Option<Self> {
            let mut objects = Vec::new();
            for (index, path) in ["/etc/nginx/nginx.conf", "/etc/hosts", "/var/lib/app/db"]
                .iter()
                .enumerate()
            {
                let inode = i128::try_from(4000 + index).ok()?;
                let name = path.rsplit('/').next().unwrap_or(path);
                let record = record(
                    "ono.file/1",
                    &[
                        ("path", Value::string(path)),
                        ("name", Value::string(name)),
                        ("kind", Value::string("file")),
                        ("device", Value::Int(64)),
                        ("inode", Value::Int(inode)),
                    ],
                )?;
                objects.push((project(&record, SpatialType::File)?, "ono.file/1"));
            }
            for (pid, name) in [(1842, "nginx"), (1843, "nginx"), (77, "app")] {
                let record = record(
                    "ono.process/1",
                    &[
                        ("pid", Value::Int(pid)),
                        ("name", Value::string(name)),
                        ("state", Value::string("sleeping")),
                        ("started", Value::string("2026-08-10T06:12:00Z")),
                    ],
                )?;
                objects.push((project(&record, SpatialType::Process)?, "ono.process/1"));
            }
            for name in ["nginx.service", "app.service"] {
                let record = record(
                    "ono.service/1",
                    &[
                        ("name", Value::string(name)),
                        ("state", Value::string("running")),
                        ("provider", Value::string("systemd")),
                    ],
                )?;
                objects.push((project(&record, SpatialType::Service)?, "ono.service/1"));
            }
            Some(Self { objects })
        }

        pub(super) fn objects(&self) -> &[(SpatialObject, &'static str)] {
            &self.objects
        }

        /// A relation between two of the world's objects, where one of the declared relations
        /// joins their types: a process opens a file, a service controls a process.
        pub(super) fn edge(
            &self,
            left: u8,
            right: u8,
            confidence: Confidence,
        ) -> Option<RelationshipEdge> {
            let (source_schema, relation, target_schema) = if left & 1 == 0 {
                ("ono.process/1", "process.opened_file", "ono.file/1")
            } else {
                ("ono.service/1", "service.controls_process", "ono.process/1")
            };
            let of = |schema: &str, byte: u8| {
                let candidates: Vec<&SpatialObject> = self
                    .objects
                    .iter()
                    .filter(|(_, held)| *held == schema)
                    .map(|(object, _)| object)
                    .collect();
                candidates
                    .get(usize::from(byte) % candidates.len().max(1))
                    .copied()
            };
            let source = of(source_schema, left >> 1)?;
            let target = of(target_schema, right)?;
            Some(RelationshipEdge::new(
                source.spatial_id().clone(),
                target.spatial_id().clone(),
                RelationType::new(relation)?,
                confidence,
                Provenance::local("systemd", SchemaId::new("ono.service", 1)),
                NOW,
            ))
        }

        pub(super) fn index(&self, edges: &[RelationshipEdge]) -> SpatialIndex {
            let mut index = SpatialIndex::new(FreshnessPolicy::default());
            for (object, _) in &self.objects {
                let _ = index.register(object.clone(), NOW);
            }
            for edge in edges {
                index.record_edge(edge.clone());
            }
            index
        }
    }

    pub(super) fn target_of(object: &SpatialObject, schema: &str) -> FrozenTarget {
        FrozenTarget::new(
            schema,
            object.spatial_id().as_str(),
            object.display_name().to_owned(),
        )
        .at_place(object.spatial_id().as_str())
    }

    fn record(schema: &str, fields: &[(&str, Value)]) -> Option<RecordValue> {
        let id: SchemaId = schema.parse().ok()?;
        let contract = builtin_schemas().get(&id)?;
        let mut builder = RecordValue::builder(contract, Provenance::local("ono-fuzz", id));
        for (name, value) in fields {
            builder = builder.set(name, value.clone()).ok()?;
        }
        Some(builder.build())
    }

    fn project(record: &RecordValue, kind: SpatialType) -> Option<SpatialObject> {
        let scope = SpatialScope::host("fuzzbox", BootIdentity::new("fuzzbox", "boot-a"));
        Projection::new(scope, NOW).project_as(record, kind).ok()
    }
}

// --- recovery metadata -------------------------------------------------------------------------

/// The recovery-plan record the plan store keeps (§36.1, §46.5) and the manifest a file archive
/// keeps (§15.1). Both are read back after a crash, and both must read back as what was written.
pub(crate) fn recovery_metadata(data: &[u8]) {
    let Some((selector, rest)) = data.split_first() else {
        return;
    };
    let text = String::from_utf8_lossy(rest);
    if selector & 1 == 1 {
        if let Ok(manifest) = ono_recovery_files::Manifest::decode(&text) {
            let again = ono_recovery_files::Manifest::decode(&manifest.encode());
            assert!(
                again.as_ref().is_ok_and(|again| again == &manifest),
                "§15.1: a file archive manifest does not read back as itself: {manifest:?} became \
                 {again:?}"
            );
        }
        return;
    }
    let Ok(ono_value::Value::Record(record)) =
        ono_value::from_json_str(&text, ono_value::builtin_schemas())
    else {
        return;
    };
    let Ok(plan) = ChangePlan::draft(
        Intent::new("recover the configuration", "recover @a82f"),
        "ono-fuzz",
        jiff::Timestamp::UNIX_EPOCH,
    )
    .seal(jiff::Timestamp::UNIX_EPOCH) else {
        return;
    };
    let Ok(recovery) = recovery_plan_from_record(&record, plan.clone()) else {
        return;
    };
    // §24.5: a recovery that would destroy newer history is gated unless the store says the gate
    // was passed, and the record is where it says so — it must survive a re-encode either way.
    if let Ok(encoded) = recovery_plan_record(&recovery) {
        let again = recovery_plan_from_record(&encoded, plan);
        assert!(
            again.as_ref().is_ok_and(|again| again == &recovery),
            "§36.1: a recovery plan read from the store does not re-encode to itself"
        );
    }
}
