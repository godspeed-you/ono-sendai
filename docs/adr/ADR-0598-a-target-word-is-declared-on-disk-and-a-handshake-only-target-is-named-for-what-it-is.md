# ADR-0598: A target word is declared on disk, and a handshake-only target is named for what it is

- Status: accepted
- Date: 2026-09-07
- Spec refs: §31.22, §31.23, §31.64, §31.65, §31.68, §31.69; v0.4 §36.1;
  `docs/contracts/kuang/contributions.v1.yaml`; ADR-0282, ADR-0582, ADR-0584; the Kubernetes
  provider's ADR-0010 (by reference)
- Decided by: agent (autonomous)

## Context

The shell registers an invocable target from the on-disk `contributions.targets` document, so
that `get pod`, its help page and its completion answer before the package has run (§31.68,
ADR-0282). The handshake then contributes targets too, and the supervisor validates and mounts a
provider for each. A target present at the handshake and absent from disk was **accepted
silently**: the provider existed, the word did not, and nothing said so. The reverse
disagreement — a word on disk that the handshake does not answer — was already refused with a
message.

The Kubernetes provider found the asymmetry and asked the larger question behind it: could a
resource kind a cluster serves, discovered *after* the package was built, become a target word
of its own — `get sprocket` rather than `get k8s-resource --kind Sprocket`? That would need a
target registered at handshake time or later, from a document nobody wrote.

## Decision

**The vocabulary of target words stays on disk. A target the handshake contributes without an
on-disk declaration is answerable through the provider path and is not a word; `load plugin`
says so by name. A discovered kind is not registered as a word.**

Three reasons, in order of weight:

1. **§31.68 is deliberate.** The registry placeholders exist so that `help`, completion and
   `explain` are stable before a package runs and identical across sessions. A word that appears
   when a package loads, and a different set of words when it loads against a different cluster,
   is a shell whose language depends on which server answered last — §31.69's reproducibility
   promise broken at the parser.
2. **Collision rules would be decided per session.** Two packages, or one package against two
   clusters, contributing `get widget` would need a resolution §31.65 requires to be *explicit*,
   and there is nobody at handshake time to be explicit with.
3. **The floor already reaches every kind.** A discovered kind is typeable as an option of a
   declared word, with help, completion and a schema (the Kubernetes provider's ADR-0010). What a
   dynamic word would add is a shorter spelling, and the price is the language's stability.

What changes: `load plugin` reports, beside the commands it loaded, the targets the handshake
contributed that the package did not declare on disk — `answerable, not spellable: echo-zone
(declared at handshake; not in contributions.targets)` — so the asymmetry is visible where it
arises. The handshake targets still mount a provider and still become kinds of place
(ADR-0584), because those are questions the handshake is the right source for; the *word* is
the document's.

## Consequences

A package author who forgets a target in the document sees it named at load rather than
discovering that `get <word>` does not resolve. The Kubernetes provider keeps `k8s-resource
--kind <Kind>` as the route to a kind invented after the build, and its ADR-0010 stands.

The dynamic word remains open as a *language* decision rather than a mechanism: if a later
specification gives the shell a way to declare a word's collision and lifecycle rules, the
registry can be extended then. Nothing here forecloses it.

`crates/ono-cli/tests/plugin_targets.rs::should_name_a_handshake_target_the_document_does_not_declare_at_load`
encodes the visible half.

## Alternatives considered

**Register handshake-only targets into the command registry at load.** Rejected for reasons 1
and 2: it makes the language session-dependent and leaves collisions to load order, which
§31.65 forbids.

**Refuse a package whose handshake contributes a target the document does not declare.**
Rejected: the handshake is the right source for a provider and a kind of place, and every
existing package contributes more targets than it spells — refusing would break them for a
defect that a report makes harmless.

**A `commands.register` host call for a running package.** Rejected as the same decision with
more surface: a word registered mid-session by package code is a word with no document behind it.
