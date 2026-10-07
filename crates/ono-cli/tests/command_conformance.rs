//! The command conformance suite of issue #149: every documented example a command's contracts
//! let run hermetically, run through the real `ono` in a scratch environment, with every value it
//! produces held to the command's declared `output` (ADR-0935). Generated from
//! `docs/contracts/commands/*.yaml`, `verbs.yaml`, `capabilities.yaml`, `schemas/deferred.yaml`
//! and `conformance/command_examples.yaml` by `cargo xtask conformance`.
//!
//! Do not edit by hand: your changes will be overwritten and the gate will fail. An example that
//! cannot run here is either skipped by its contracts or exempted in
//! `docs/contracts/conformance/command_examples.yaml`, with the reason.
//!
//! 103 examples run; 250 are not run, each for the reason listed at the end of this file.
//!
//! Commands no example of which runs (163):
//!
//! - `ono.change.plan` — exempt: plans to replace `/etc/nginx/nginx.conf` from `./nginx.conf`; a scratch directory has neither file.
//! - `ono.change-plan.inspect` — exempt: names the plan `a82f`, and a fresh state directory holds no plan.
//! - `ono.change-plan.rebase` — exempt: names the plan `a82f`, and a fresh state directory holds no plan.
//! - `ono.change-plan.resume` — runs `ono.change-plan.resume`, whose verb `resume` changes the system (verbs.yaml)
//! - `ono.change.impact` — exempt: names the plan `a82f`, and a fresh state directory holds no plan.
//! - `ono.change.protect` — runs `ono.change.protect`, whose verb `protect` changes the system (verbs.yaml)
//! - `ono.change.apply` — runs `ono.change.apply`, whose verb `apply` changes the system (verbs.yaml)
//! - `ono.change.verify` — exempt: names the plan `a82f`, and a fresh state directory holds no plan.
//! - `ono.change.recover` — exempt: names the plan `a82f`, and a fresh state directory holds no plan.
//! - `ono.recovery.inspect` — exempt: names the recovery asset `r-a82f`, and a fresh state directory holds none.
//! - `ono.recovery.remove` — runs `ono.recovery.remove`, whose verb `remove` changes the system (verbs.yaml)
//! - `ono.container.get` — runs `ono.container.get`, which declares privilege `conditional`
//! - `ono.container.start` — runs `ono.container.start`, whose verb `start` changes the system (verbs.yaml)
//! - `ono.container.stop` — runs `ono.container.stop`, whose verb `stop` changes the system (verbs.yaml)
//! - `ono.container.restart` — runs `ono.container.restart`, whose verb `restart` changes the system (verbs.yaml)
//! - `ono.container.enter` — runs `ono.container.enter`, which declares privilege `conditional`
//! - `ono.container.trace` — runs `ono.container.trace`, which declares privilege `conditional`
//! - `ono.image.get` — runs `ono.image.get`, which declares privilege `conditional`
//! - `ono.container.watch` — runs `ono.container.watch`, whose verb `watch` follows a live source that does not end (verbs.yaml)
//! - `ono.container.remove` — runs `ono.container.remove`, whose verb `remove` changes the system (verbs.yaml)
//! - `ono.container.set` — runs `ono.container.set`, whose verb `set` changes the system (verbs.yaml)
//! - `ono.data.each` — runs `ono.service.restart`, whose verb `restart` changes the system (verbs.yaml)
//! - `ono.data.measure` — runs `ono.data.measure`, which declares `ono.measure/1` — a schema schemas/deferred.yaml says a later phase writes
//! - `ono.data.from` — runs `curl`, which no command contract declares — an external program or a shell keyword, whose effects nothing states
//! - `ono.data.tail` — runs `ono.log.get`, which declares privilege `conditional`
//! - `ono.data.diff` — refers to an earlier result, which the scratch shell does not have
//! - `ono.file.get` — runs `ono.file.get`, which declares privilege `conditional`
//! - `ono.file.find` — runs `ono.file.find`, which declares privilege `conditional`
//! - `ono.file.read` — runs `ono.file.read`, which declares privilege `conditional`
//! - `ono.file.write` — runs `ono.file.write`, whose verb `write` changes the system (verbs.yaml)
//! - `ono.file.copy` — runs `ono.file.copy`, whose verb `copy` changes the system (verbs.yaml)
//! - `ono.file.move` — runs `ono.file.move`, whose verb `move` changes the system (verbs.yaml)
//! - `ono.file.remove` — runs `ono.file.find`, which declares privilege `conditional`
//! - `ono.file.set` — runs `ono.file.set`, whose verb `set` changes the system (verbs.yaml)
//! - `ono.file.trace` — runs `ono.file.trace`, which declares privilege `conditional`
//! - `ono.file.tail` — runs `ono.file.tail`, which declares privilege `conditional`
//! - `ono.file.open` — runs `ono.file.open`, whose verb `open` changes the system (verbs.yaml)
//! - `ono.file.watch` — runs `ono.file.watch`, whose verb `watch` follows a live source that does not end (verbs.yaml)
//! - `ono.file.enter` — runs `ono.file.enter`, which declares privilege `conditional`
//! - `ono.dir.get` — runs `ono.dir.get`, which declares privilege `conditional`
//! - `ono.dir.enter` — runs `ono.dir.enter`, which declares privilege `conditional`
//! - `ono.dir.remove` — runs `ono.dir.remove`, whose verb `remove` changes the system (verbs.yaml)
//! - `ono.dir.set` — runs `ono.dir.set`, whose verb `set` changes the system (verbs.yaml)
//! - `ono.env.set` — runs `ono.env.set`, whose verb `set` changes the system (verbs.yaml)
//! - `ono.user.add` — runs `ono.user.add`, whose verb `add` changes the system (verbs.yaml)
//! - `ono.user.remove` — runs `ono.user.remove`, whose verb `remove` changes the system (verbs.yaml)
//! - `ono.user.set` — runs `ono.user.set`, whose verb `set` changes the system (verbs.yaml)
//! - `ono.group.add` — runs `ono.group.add`, whose verb `add` changes the system (verbs.yaml)
//! - `ono.group.remove` — runs `ono.group.remove`, whose verb `remove` changes the system (verbs.yaml)
//! - `ono.group.set` — runs `ono.group.set`, whose verb `set` changes the system (verbs.yaml)
//! - `ono.user.watch` — runs `ono.user.watch`, whose verb `watch` follows a live source that does not end (verbs.yaml)
//! - `ono.user.trace` — runs `ono.user.trace`, which declares privilege `conditional`
//! - `ono.user.enter` — exempt: runs only as a statement of its own; in a pipeline this build implements nothing for it.
//! - `ono.group.watch` — runs `ono.group.watch`, whose verb `watch` follows a live source that does not end (verbs.yaml)
//! - `ono.group.enter` — exempt: runs only as a statement of its own; in a pipeline this build implements nothing for it.
//! - `ono.plugin.inspect` — exempt: names the package `dev.example.packet-eye`, and a fresh plugin path has none installed.
//! - `ono.plugin.install` — runs `ono.plugin.install`, whose verb `install` changes the system (verbs.yaml)
//! - `ono.plugin.remove` — runs `ono.plugin.remove`, whose verb `remove` changes the system (verbs.yaml)
//! - `ono.plugin.load` — runs `ono.plugin.load`, whose verb `load` changes the system (verbs.yaml)
//! - `ono.plugin.unload` — runs `ono.plugin.unload`, whose verb `unload` changes the system (verbs.yaml)
//! - `ono.plugin.set` — runs `ono.plugin.set`, whose verb `set` changes the system (verbs.yaml)
//! - `ono.plugin.verify` — exempt: names the package `dev.example.packet-eye`, and a fresh plugin path has none installed.
//! - `ono.permission.set` — runs `ono.permission.set`, whose verb `set` changes the system (verbs.yaml)
//! - `ono.capability.grant` — runs `ono.capability.grant`, whose verb `grant` changes the system (verbs.yaml)
//! - `ono.capability.revoke` — runs `ono.capability.revoke`, whose verb `revoke` changes the system (verbs.yaml)
//! - `ono.assistant.ask` — runs `ono.assistant.ask`, whose capability `assistant.ask` is `mutate` rather than `read`
//! - `ono.meta.help` — runs `ono.meta.help`, which declares `ono.help-page/1` — a schema schemas/deferred.yaml says a later phase writes
//! - `ono.meta.type` — runs `ono.meta.type`, which declares `ono.type-info/1` — a schema schemas/deferred.yaml says a later phase writes
//! - `ono.meta.inspect` — runs `ono.meta.inspect`, which declares `ono.inspection/1` — a schema schemas/deferred.yaml says a later phase writes
//! - `ono.meta.explain` — runs `ono.meta.explain`, which declares `ono.execution-plan/1` — a schema schemas/deferred.yaml says a later phase writes
//! - `ono.config.set` — runs `ono.config.set`, whose verb `set` changes the system (verbs.yaml)
//! - `ono.context.leave` — runs `leave`, which no command contract declares — an external program or a shell keyword, whose effects nothing states
//! - `ono.socket.get` — runs `ono.socket.get`, which declares privilege `conditional`
//! - `ono.connection.get` — runs `ono.connection.get`, which declares privilege `conditional`
//! - `ono.dns.resolve` — runs `ono.dns.resolve`, whose capability `dns.resolve` reaches the network
//! - `ono.port.test` — runs `ono.port.test`, whose capability `port.probe` is `observe` rather than `read`
//! - `ono.connection.trace` — runs `ono.connection.trace`, which declares privilege `conditional`
//! - `ono.socket.trace` — runs `ono.socket.trace`, which declares privilege `conditional`
//! - `ono.socket.watch` — runs `ono.socket.watch`, whose verb `watch` follows a live source that does not end (verbs.yaml)
//! - `ono.interface.watch` — runs `ono.interface.watch`, whose verb `watch` follows a live source that does not end (verbs.yaml)
//! - `ono.route.watch` — runs `ono.route.watch`, whose verb `watch` follows a live source that does not end (verbs.yaml)
//! - `ono.route.add` — runs `ono.route.add`, whose verb `add` changes the system (verbs.yaml)
//! - `ono.route.remove` — runs `ono.route.remove`, whose verb `remove` changes the system (verbs.yaml)
//! - `ono.route.set` — runs `ono.route.set`, whose verb `set` changes the system (verbs.yaml)
//! - `ono.interface.set` — runs `ono.interface.set`, whose verb `set` changes the system (verbs.yaml)
//! - `ono.interface.start` — runs `ono.interface.start`, whose verb `start` changes the system (verbs.yaml)
//! - `ono.interface.stop` — runs `ono.interface.stop`, whose verb `stop` changes the system (verbs.yaml)
//! - `ono.interface.add` — runs `ono.interface.add`, whose verb `add` changes the system (verbs.yaml)
//! - `ono.interface.remove` — runs `ono.interface.remove`, whose verb `remove` changes the system (verbs.yaml)
//! - `ono.interface.trace` — runs `ono.interface.trace`, which declares privilege `conditional`
//! - `ono.interface.enter` — exempt: runs only as a statement of its own; in a pipeline this build implements nothing for it.
//! - `ono.socket.stop` — runs `ono.socket.get`, which declares privilege `conditional`
//! - `ono.socket.enter` — runs `ono.socket.enter`, which declares privilege `conditional`
//! - `ono.package.add` — runs `ono.package.add`, whose verb `add` changes the system (verbs.yaml)
//! - `ono.package.remove` — runs `ono.package.remove`, whose verb `remove` changes the system (verbs.yaml)
//! - `ono.package.set` — runs `ono.package.set`, whose verb `set` changes the system (verbs.yaml)
//! - `ono.package-source.refresh` — runs `ono.package-source.refresh`, whose verb `refresh` changes the system (verbs.yaml)
//! - `ono.process.inspect` — runs `ono.process.inspect`, which declares privilege `conditional`
//! - `ono.process.watch` — runs `ono.process.watch`, whose verb `watch` follows a live source that does not end (verbs.yaml)
//! - `ono.process.stop` — runs `ono.process.stop`, whose verb `stop` changes the system (verbs.yaml)
//! - `ono.process.kill` — runs `ono.process.kill`, whose verb `kill` changes the system (verbs.yaml)
//! - `ono.process.trace` — runs `trace`, which no command contract declares — an external program or a shell keyword, whose effects nothing states
//! - `ono.process.enter` — exempt: runs only as a statement of its own; in a pipeline this build implements nothing for it.
//! - `ono.process.set` — runs `ono.process.set`, whose verb `set` changes the system (verbs.yaml)
//! - `ono.signal.send` — runs `ono.signal.send`, whose verb `send` changes the system (verbs.yaml)
//! - `ono.host.link` — runs `ono.host.link`, whose verb `link` changes the system (verbs.yaml)
//! - `ono.link.detach` — runs `ono.link.detach`, whose capability `link.manage` is `mutate` rather than `read`
//! - `ono.link.enter` — exempt: runs only as a statement of its own; in a pipeline this build implements nothing for it.
//! - `ono.host.test` — runs `ono.host.test`, whose capability `host.probe` is `observe` rather than `read`
//! - `ono.host.connect` — runs `ono.host.connect`, whose verb `connect` changes the system (verbs.yaml)
//! - `ono.host.trace` — exempt: names the host `prod-db`, which a fresh configuration does not know.
//! - `ono.link.watch` — runs `ono.link.watch`, whose verb `watch` follows a live source that does not end (verbs.yaml)
//! - `ono.link.add` — runs `ono.link.add`, whose verb `add` changes the system (verbs.yaml)
//! - `ono.link.remove` — runs `ono.link.remove`, whose verb `remove` changes the system (verbs.yaml)
//! - `ono.link.set` — runs `ono.link.set`, whose verb `set` changes the system (verbs.yaml)
//! - `ono.link.rename` — runs `ono.link.rename`, whose verb `rename` changes the system (verbs.yaml)
//! - `ono.host.watch` — runs `ono.host.watch`, whose verb `watch` follows a live source that does not end (verbs.yaml)
//! - `ono.host.add` — runs `ono.host.add`, whose verb `add` changes the system (verbs.yaml)
//! - `ono.host.remove` — runs `ono.host.remove`, whose verb `remove` changes the system (verbs.yaml)
//! - `ono.host.set` — runs `ono.host.set`, whose verb `set` changes the system (verbs.yaml)
//! - `ono.link.trace` — exempt: names the link `prod-db`, which a fresh configuration does not hold.
//! - `ono.host-key.add` — runs `ono.host-key.add`, whose verb `add` changes the system (verbs.yaml)
//! - `ono.host-key.set` — runs `ono.host-key.set`, whose verb `set` changes the system (verbs.yaml)
//! - `ono.host-key.remove` — runs `ono.host-key.remove`, whose verb `remove` changes the system (verbs.yaml)
//! - `ono.client-key.add` — runs `ono.client-key.add`, whose verb `add` changes the system (verbs.yaml)
//! - `ono.client-key.set` — runs `ono.client-key.set`, whose verb `set` changes the system (verbs.yaml)
//! - `ono.client-key.remove` — runs `ono.client-key.remove`, whose verb `remove` changes the system (verbs.yaml)
//! - `ono.service.start` — runs `ono.service.start`, whose verb `start` changes the system (verbs.yaml)
//! - `ono.service.stop` — runs `ono.service.stop`, whose verb `stop` changes the system (verbs.yaml)
//! - `ono.service.restart` — runs `ono.service.restart`, whose verb `restart` changes the system (verbs.yaml)
//! - `ono.service.watch` — runs `ono.service.watch`, whose verb `watch` follows a live source that does not end (verbs.yaml)
//! - `ono.service.trace` — runs `ono.service.trace`, which declares privilege `conditional`
//! - `ono.service.enter` — exempt: runs only as a statement of its own; in a pipeline this build implements nothing for it.
//! - `ono.service.set` — runs `ono.service.set`, whose verb `set` changes the system (verbs.yaml)
//! - `ono.log.get` — runs `ono.log.get`, which declares privilege `conditional`
//! - `ono.journal.get` — runs `ono.journal.get`, which declares privilege `conditional`
//! - `ono.journal.tail` — runs `ono.journal.tail`, which declares privilege `conditional`
//! - `ono.place.find` — runs `ono.place.find`, which declares privilege `conditional`
//! - `ono.place.near` — runs `ono.place.near`, which declares privilege `conditional`
//! - `ono.place.enter` — runs `ono.place.enter`, which declares privilege `conditional`
//! - `ono.place.follow` — runs `ono.place.follow`, which declares privilege `conditional`
//! - `ono.place.map` — runs `ono.place.map`, which declares privilege `conditional`
//! - `ono.place.back` — runs `ono.place.follow`, which declares privilege `conditional`
//! - `ono.place.up` — runs `ono.place.enter`, which declares privilege `conditional`
//! - `ono.place.jump` — runs `ono.place.jump`, which declares privilege `conditional`
//! - `ono.place.unpin` — exempt: removes the pin `edge-proxy`, and a fresh session has pinned nothing.
//! - `ono.filesystem.get` — runs `ono.filesystem.get`, which declares privilege `conditional`
//! - `ono.filesystem.mount` — runs `ono.filesystem.mount`, whose verb `mount` changes the system (verbs.yaml)
//! - `ono.filesystem.unmount` — runs `ono.filesystem.unmount`, whose verb `unmount` changes the system (verbs.yaml)
//! - `ono.mount.set` — runs `ono.mount.set`, whose verb `set` changes the system (verbs.yaml)
//! - `ono.mount.add` — runs `ono.mount.add`, whose verb `add` changes the system (verbs.yaml)
//! - `ono.mount.remove` — runs `ono.mount.remove`, whose verb `remove` changes the system (verbs.yaml)
//! - `ono.mount.start` — runs `ono.mount.start`, whose verb `start` changes the system (verbs.yaml)
//! - `ono.mount.stop` — runs `ono.mount.stop`, whose verb `stop` changes the system (verbs.yaml)
//! - `ono.mount.watch` — runs `ono.mount.watch`, whose verb `watch` follows a live source that does not end (verbs.yaml)
//! - `ono.mount.trace` — runs `ono.mount.trace`, which declares privilege `conditional`
//! - `ono.mount.enter` — exempt: runs only as a statement of its own; in a pipeline this build implements nothing for it.
//! - `ono.temporal.at` — refers to `@e42`, a result or event of a session the scratch shell does not have
//! - `ono.temporal.present` — exempt: runs `printf`, an external program that writes to the terminal before the action result, so its output is not one inspectable document.
//! - `ono.event.inspect` — refers to `@e42`, a result or event of a session the scratch shell does not have
//! - `ono.recorder.start` — runs `ono.recorder.start`, whose verb `start` changes the system (verbs.yaml)
//! - `ono.recorder.stop` — runs `ono.recorder.stop`, whose verb `stop` changes the system (verbs.yaml)
//! - `ono.temporal-history.remove` — runs `ono.temporal-history.remove`, whose verb `remove` changes the system (verbs.yaml)

// The examples are the full product's. The core build of #127 answers the rest as unavailable,
// so this suite is the full build's (ADR-0910, ADR-0925).
#![cfg(feature = "full")]
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md §16)"
)]

mod conformance_harness;

use conformance_harness as harness;

/// `get plan`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_change_plan_get_declares_when_example_1_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.change-plan.get",
        example: "get plan",
        output: "stream<ono.change-plan/1>",
    });
}

/// `get plan --state failed`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_change_plan_get_declares_when_example_3_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.change-plan.get",
        example: "get plan --state failed",
        output: "stream<ono.change-plan/1>",
    });
}

/// `get recovery`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_recovery_get_declares_when_example_1_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.recovery.get",
        example: "get recovery",
        output: "stream<ono.recovery-asset/1>",
    });
}

/// `get process | where cpu > 20`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_data_where_declares_when_example_1_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.data.where",
        example: "get process | where cpu > 20",
        output: "stream<any>",
    });
}

/// `get process | where cpu > 20 and user.name != "root"`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_data_where_declares_when_example_2_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.data.where",
        example: "get process | where cpu > 20 and user.name != \"root\"",
        output: "stream<any>",
    });
}

/// `get process | select pid name memory`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_data_select_declares_when_example_1_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.data.select",
        example: "get process | select pid name memory",
        output: "stream<record>",
    });
}

/// `get process | select pid name {mem_mb: memory / 1MiB}`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_data_select_declares_when_example_2_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.data.select",
        example: "get process | select pid name {mem_mb: memory / 1MiB}",
        output: "stream<record>",
    });
}

/// `get process | sort memory desc`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_data_sort_declares_when_example_1_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.data.sort",
        example: "get process | sort memory desc",
        output: "stream<any>",
    });
}

/// `get process | sort cpu desc | take 10`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_data_sort_declares_when_example_2_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.data.sort",
        example: "get process | sort cpu desc | take 10",
        output: "stream<any>",
    });
}

/// `get process | group user.name`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_data_group_declares_when_example_1_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.data.group",
        example: "get process | group user.name",
        output: "stream<record>",
    });
}

/// `get process | sort cpu desc | take 10`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_data_take_declares_when_example_1_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.data.take",
        example: "get process | sort cpu desc | take 10",
        output: "stream<any>",
    });
}

/// `get process | sort cpu desc | skip 1 | take 5`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_data_skip_declares_when_example_1_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.data.skip",
        example: "get process | sort cpu desc | skip 1 | take 5",
        output: "stream<any>",
    });
}

/// `get process | select memory | reduce $acc + @ --initial 0`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_data_reduce_declares_when_example_1_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.data.reduce",
        example: "get process | select memory | reduce $acc + @ --initial 0",
        output: "value",
    });
}

/// `get process | count`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_data_count_declares_when_example_1_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.data.count",
        example: "get process | count",
        output: "int",
    });
}

/// `get process | to json`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_data_to_declares_when_example_1_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.data.to",
        example: "get process | to json",
        output: "string | bytes",
    });
}

/// `get process | to json --pretty`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_data_to_declares_when_example_2_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.data.to",
        example: "get process | to json --pretty",
        output: "string | bytes",
    });
}

/// `get mount | select target | to text`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_data_to_declares_when_example_4_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.data.to",
        example: "get mount | select target | to text",
        output: "string | bytes",
    });
}

/// `get process | format table`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_data_format_declares_when_example_1_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.data.format",
        example: "get process | format table",
        output: "string",
    });
}

/// `get process | format table --columns ["pid", "name", "memory"]`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_data_format_declares_when_example_2_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.data.format",
        example: "get process | format table --columns [\"pid\", \"name\", \"memory\"]",
        output: "string",
    });
}

/// `get process | view table`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_data_view_declares_when_example_1_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.data.view",
        example: "get process | view table",
        output: "null",
    });
}

/// `get process | join (get process | where cpu > 1) --on pid`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_data_join_declares_when_example_1_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.data.join",
        example: "get process | join (get process | where cpu > 1) --on pid",
        output: "stream<record>",
    });
}

/// `get user`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_user_get_declares_when_example_1_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.user.get",
        example: "get user",
        output: "stream<ono.user/1>",
    });
}

/// `get user postgres`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_user_get_declares_when_example_2_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.user.get",
        example: "get user postgres",
        output: "stream<ono.user/1>",
    });
}

/// `get group`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_group_get_declares_when_example_1_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.group.get",
        example: "get group",
        output: "stream<ono.group/1>",
    });
}

/// `get group | where "postgres" in members`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_group_get_declares_when_example_2_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.group.get",
        example: "get group | where \"postgres\" in members",
        output: "stream<ono.group/1>",
    });
}

/// `get session`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_session_get_declares_when_example_1_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.session.get",
        example: "get session",
        output: "stream<ono.session/1>",
    });
}

/// `get env`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_env_get_declares_when_example_1_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.env.get",
        example: "get env",
        output: "stream<ono.env-var/1>",
    });
}

/// `get env PATH`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_env_get_declares_when_example_2_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.env.get",
        example: "get env PATH",
        output: "stream<ono.env-var/1>",
    });
}

/// `get plugin`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_plugin_get_declares_when_example_1_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.plugin.get",
        example: "get plugin",
        output: "stream<ono.plugin/1>",
    });
}

/// `get plugin | where state == loaded`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_plugin_get_declares_when_example_2_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.plugin.get",
        example: "get plugin | where state == loaded",
        output: "stream<ono.plugin/1>",
    });
}

/// `find plugin postgres`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_plugin_find_declares_when_example_1_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.plugin.find",
        example: "find plugin postgres",
        output: "stream<ono.plugin-package/1>",
    });
}

/// `get permission`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_permission_get_declares_when_example_3_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.permission.get",
        example: "get permission",
        output: "stream<ono.permission/1>",
    });
}

/// `get capability`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_capability_get_declares_when_example_1_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.capability.get",
        example: "get capability",
        output: "stream<ono.capability-grant/1>",
    });
}

/// `get capability --plugin dev.example.packet-eye`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_capability_get_declares_when_example_2_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.capability.get",
        example: "get capability --plugin dev.example.packet-eye",
        output: "stream<ono.capability-grant/1>",
    });
}

/// `get assistant`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_assistant_get_declares_when_example_1_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.assistant.get",
        example: "get assistant",
        output: "stream<ono.assistant/1>",
    });
}

/// `get model`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_model_get_declares_when_example_1_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.model.get",
        example: "get model",
        output: "stream<ono.model-provider/1>",
    });
}

/// `get finding | where severity >= medium`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_finding_get_declares_when_example_1_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.finding.get",
        example: "get finding | where severity >= medium",
        output: "stream<ono.finding/1>",
    });
}

/// `get audit --plugin dev.example.packet-eye`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_audit_get_declares_when_example_1_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.audit.get",
        example: "get audit --plugin dev.example.packet-eye",
        output: "stream<ono.plugin-audit-event/1>",
    });
}

/// `get command`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_command_get_declares_when_example_1_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.command.get",
        example: "get command",
        output: "stream<ono.command/1>",
    });
}

/// `get command --verb trace`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_command_get_declares_when_example_2_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.command.get",
        example: "get command --verb trace",
        output: "stream<ono.command/1>",
    });
}

/// `find command "listening ports"`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_command_find_declares_when_example_1_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.command.find",
        example: "find command \"listening ports\"",
        output: "stream<ono.command/1>",
    });
}

/// `resolve command ls`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_command_resolve_declares_when_example_1_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.command.resolve",
        example: "resolve command ls",
        output: "ono.command/1",
    });
}

/// `get config`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_config_get_declares_when_example_1_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.config.get",
        example: "get config",
        output: "stream<ono.config-setting/1> | stream<ono.error/1>",
    });
}

/// `get config render.`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_config_get_declares_when_example_2_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.config.get",
        example: "get config render.",
        output: "stream<ono.config-setting/1> | stream<ono.error/1>",
    });
}

/// `get config --problems`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_config_get_declares_when_example_3_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.config.get",
        example: "get config --problems",
        output: "stream<ono.config-setting/1> | stream<ono.error/1>",
    });
}

/// `inspect limits`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_limits_inspect_declares_when_example_1_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.limits.inspect",
        example: "inspect limits",
        output: "stream<ono.limit/1>",
    });
}

/// `inspect limits limits.materialize_bytes`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_limits_inspect_declares_when_example_2_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.limits.inspect",
        example: "inspect limits limits.materialize_bytes",
        output: "stream<ono.limit/1>",
    });
}

/// `get context`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_context_get_declares_when_example_1_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.context.get",
        example: "get context",
        output: "stream<ono.context/1>",
    });
}

/// `get interface`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_interface_get_declares_when_example_1_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.interface.get",
        example: "get interface",
        output: "stream<ono.interface/1>",
    });
}

/// `get interface | where state == up`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_interface_get_declares_when_example_2_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.interface.get",
        example: "get interface | where state == up",
        output: "stream<ono.interface/1>",
    });
}

/// `get route`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_route_get_declares_when_example_1_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.route.get",
        example: "get route",
        output: "stream<ono.route/1>",
    });
}

/// `get route | where destination == null`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_route_get_declares_when_example_2_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.route.get",
        example: "get route | where destination == null",
        output: "stream<ono.route/1>",
    });
}

/// `get neighbor`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_neighbor_get_declares_when_example_1_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.neighbor.get",
        example: "get neighbor",
        output: "stream<ono.neighbor/1>",
    });
}

/// `get neighbor | where state == reachable`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_neighbor_get_declares_when_example_2_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.neighbor.get",
        example: "get neighbor | where state == reachable",
        output: "stream<ono.neighbor/1>",
    });
}

/// `trace route 0.0.0.0/0`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_route_trace_declares_when_example_1_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.route.trace",
        example: "trace route 0.0.0.0/0",
        output: "ono.graph/1",
    });
}

/// `get package`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_package_get_declares_when_example_1_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.package.get",
        example: "get package",
        output: "stream<ono.package/1>",
    });
}

/// `get package nginx`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_package_get_declares_when_example_2_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.package.get",
        example: "get package nginx",
        output: "stream<ono.package/1>",
    });
}

/// `find package postgres`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_package_find_declares_when_example_1_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.package.find",
        example: "find package postgres",
        output: "stream<ono.package/1>",
    });
}

/// `get package-source`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_package_source_get_declares_when_example_1_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.package-source.get",
        example: "get package-source",
        output: "stream<ono.package-source/1>",
    });
}

/// `get process`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_process_get_declares_when_example_1_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.process.get",
        example: "get process",
        output: "stream<ono.process/1>",
    });
}

/// `get process | where cpu > 20`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_process_get_declares_when_example_3_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.process.get",
        example: "get process | where cpu > 20",
        output: "stream<ono.process/1>",
    });
}

/// `get process --sample 500ms | where cpu > 20`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_process_get_declares_when_example_4_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.process.get",
        example: "get process --sample 500ms | where cpu > 20",
        output: "stream<ono.process/1>",
    });
}

/// `get job`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_job_get_declares_when_example_1_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.job.get",
        example: "get job",
        output: "stream<ono.job/1>",
    });
}

/// `get job | where state == running`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_job_get_declares_when_example_2_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.job.get",
        example: "get job | where state == running",
        output: "stream<ono.job/1>",
    });
}

/// `get host`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_host_get_declares_when_example_1_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.host.get",
        example: "get host",
        output: "stream<ono.host/1>",
    });
}

/// `get link`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_link_get_declares_when_example_1_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.link.get",
        example: "get link",
        output: "stream<ono.link/1>",
    });
}

/// `get host-key`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_host_key_get_declares_when_example_1_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.host-key.get",
        example: "get host-key",
        output: "stream<ono.host-key/1>",
    });
}

/// `get client-key`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_client_key_get_declares_when_example_1_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.client-key.get",
        example: "get client-key",
        output: "stream<ono.client-key/1>",
    });
}

/// `get service`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_service_get_declares_when_example_1_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.service.get",
        example: "get service",
        output: "stream<ono.service/1>",
    });
}

/// `get service nginx`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_service_get_declares_when_example_2_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.service.get",
        example: "get service nginx",
        output: "stream<ono.service/1>",
    });
}

/// `get service | where state == failed`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_service_get_declares_when_example_3_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.service.get",
        example: "get service | where state == failed",
        output: "stream<ono.service/1>",
    });
}

/// `look`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_place_look_declares_when_example_1_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.place.look",
        example: "look",
        output: "ono.place-view/1 | string",
    });
}

/// `look --json`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_place_look_declares_when_example_2_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.place.look",
        example: "look --json",
        output: "ono.place-view/1 | string",
    });
}

/// `look --all`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_place_look_declares_when_example_3_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.place.look",
        example: "look --all",
        output: "ono.place-view/1 | string",
    });
}

/// `map links`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_place_map_links_declares_when_example_1_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.place.map-links",
        example: "map links",
        output: "ono.spatial-map/1 | string",
    });
}

/// `map links --json`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_place_map_links_declares_when_example_2_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.place.map-links",
        example: "map links --json",
        output: "ono.spatial-map/1 | string",
    });
}

/// `home`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_place_home_declares_when_example_1_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.place.home",
        example: "home",
        output: "null",
    });
}

/// `home; look`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_place_home_declares_when_example_2_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.place.home",
        example: "home; look",
        output: "null",
    });
}

/// `trail`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_place_trail_declares_when_example_1_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.place.trail",
        example: "trail",
        output: "stream<ono.navigation-step/1> | string",
    });
}

/// `trail --json`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_place_trail_declares_when_example_2_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.place.trail",
        example: "trail --json",
        output: "stream<ono.navigation-step/1> | string",
    });
}

/// `trail --compact`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_place_trail_declares_when_example_3_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.place.trail",
        example: "trail --compact",
        output: "stream<ono.navigation-step/1> | string",
    });
}

/// `pin`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_place_pin_declares_when_example_1_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.place.pin",
        example: "pin",
        output: "ono.spatial-place/1",
    });
}

/// `pin --name edge-proxy`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_place_pin_declares_when_example_2_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.place.pin",
        example: "pin --name edge-proxy",
        output: "ono.spatial-place/1",
    });
}

/// `get mount`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_mount_get_declares_when_example_1_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.mount.get",
        example: "get mount",
        output: "stream<ono.mount/1>",
    });
}

/// `get mount | where read_only`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_mount_get_declares_when_example_2_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.mount.get",
        example: "get mount | where read_only",
        output: "stream<ono.mount/1>",
    });
}

/// `get device`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_device_get_declares_when_example_1_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.device.get",
        example: "get device",
        output: "stream<ono.device/1>",
    });
}

/// `now`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_temporal_now_declares_when_example_1_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.temporal.now",
        example: "now",
        output: "ono.temporal-context/1",
    });
}

/// `timeline`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_temporal_timeline_declares_when_example_1_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.temporal.timeline",
        example: "timeline",
        output: "stream<ono.temporal-event/1>",
    });
}

/// `timeline --since 30m`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_temporal_timeline_declares_when_example_2_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.temporal.timeline",
        example: "timeline --since 30m",
        output: "stream<ono.temporal-event/1>",
    });
}

/// `timeline service nginx`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_temporal_timeline_declares_when_example_3_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.temporal.timeline",
        example: "timeline service nginx",
        output: "stream<ono.temporal-event/1>",
    });
}

/// `timeline --kind object.changed`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_temporal_timeline_declares_when_example_4_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.temporal.timeline",
        example: "timeline --kind object.changed",
        output: "stream<ono.temporal-event/1>",
    });
}

/// `timeline --view`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_temporal_timeline_declares_when_example_5_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.temporal.timeline",
        example: "timeline --view",
        output: "stream<ono.temporal-event/1>",
    });
}

/// `timeline --since 1h | where kind == "object.changed"`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_temporal_timeline_declares_when_example_6_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.temporal.timeline",
        example: "timeline --since 1h | where kind == \"object.changed\"",
        output: "stream<ono.temporal-event/1>",
    });
}

/// `changes --since 10m`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_temporal_changes_declares_when_example_1_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.temporal.changes",
        example: "changes --since 10m",
        output: "stream<ono.temporal-change/1>",
    });
}

/// `changes service nginx --since 1h`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_temporal_changes_declares_when_example_2_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.temporal.changes",
        example: "changes service nginx --since 1h",
        output: "stream<ono.temporal-change/1>",
    });
}

/// `changes --since 12:00 --until 12:30`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_temporal_changes_declares_when_example_3_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.temporal.changes",
        example: "changes --since 12:00 --until 12:30",
        output: "stream<ono.temporal-change/1>",
    });
}

/// `changes --since 30m | group subject.object_type`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_temporal_changes_declares_when_example_4_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.temporal.changes",
        example: "changes --since 30m | group subject.object_type",
        output: "stream<ono.temporal-change/1>",
    });
}

/// `why service nginx`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_temporal_why_declares_when_example_1_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.temporal.why",
        example: "why service nginx",
        output: "ono.causal-explanation/1",
    });
}

/// `why field state`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_temporal_why_declares_when_example_3_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.temporal.why",
        example: "why field state",
        output: "ono.causal-explanation/1",
    });
}

/// `find event 'kind == "action.failed"'`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_event_find_declares_when_example_1_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.event.find",
        example: "find event 'kind == \"action.failed\"'",
        output: "stream<ono.temporal-event/1>",
    });
}

/// `find event --since 1h`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_event_find_declares_when_example_2_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.event.find",
        example: "find event --since 1h",
        output: "stream<ono.temporal-event/1>",
    });
}

/// `find event 'kind == "object.appeared"' | take 20`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_event_find_declares_when_example_3_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.event.find",
        example: "find event 'kind == \"object.appeared\"' | take 20",
        output: "stream<ono.temporal-event/1>",
    });
}

/// `get recorder`
#[rustfmt::skip]
#[test]
fn should_produce_what_ono_recorder_get_declares_when_example_1_runs() {
    harness::assert_example_conforms(&harness::ExampleCase {
        command: "ono.recorder.get",
        example: "get recorder",
        output: "ono.recorder-status/1",
    });
}

// Not run, with the reason the contracts or docs/contracts/conformance/command_examples.yaml give:
//
// - `ono.change.plan` `plan restart service nginx` — exempt: plans against the unit `nginx`, which a test host need not have.
// - `ono.change.plan` `plan update package openssl --protection require` — exempt: spells v0.6 §3.1's intent `update package`, and no verb `update` exists (verbs.yaml); the plan refuses it as not plannable. A gap between the specification's illustration and the verb vocabulary, recorded in ADR-0935, not a property of the test host.
// - `ono.change.plan` `get service | where state == failed | plan restart service` — exempt: plans against the failed units of the host; a healthy test host has none, and a plan over nothing is refused rather than empty.
// - `ono.change.plan` `plan copy file ./nginx.conf /etc/nginx/nginx.conf` — exempt: plans to replace `/etc/nginx/nginx.conf` from `./nginx.conf`; a scratch directory has neither file.
// - `ono.change-plan.get` `get plan a82f` — exempt: names the plan `a82f`, and a fresh state directory holds no plan.
// - `ono.change-plan.get` `get plan a82f --revision 1` — exempt: names the plan `a82f`, and a fresh state directory holds no plan.
// - `ono.change-plan.inspect` `inspect plan a82f` — exempt: names the plan `a82f`, and a fresh state directory holds no plan.
// - `ono.change-plan.inspect` `inspect plan a82f --resolution` — exempt: names the plan `a82f`, and a fresh state directory holds no plan.
// - `ono.change-plan.inspect` `inspect plan a82f --protection` — exempt: names the plan `a82f`, and a fresh state directory holds no plan.
// - `ono.change-plan.rebase` `rebase plan a82f` — exempt: names the plan `a82f`, and a fresh state directory holds no plan.
// - `ono.change-plan.resume` `resume plan a82f --confirm` — runs `ono.change-plan.resume`, whose verb `resume` changes the system (verbs.yaml)
// - `ono.change.impact` `impact a82f` — exempt: names the plan `a82f`, and a fresh state directory holds no plan.
// - `ono.change.impact` `impact a82f --depth 3` — exempt: names the plan `a82f`, and a fresh state directory holds no plan.
// - `ono.change.protect` `protect a82f --confirm` — runs `ono.change.protect`, whose verb `protect` changes the system (verbs.yaml)
// - `ono.change.apply` `apply a82f` — runs `ono.change.apply`, whose verb `apply` changes the system (verbs.yaml)
// - `ono.change.apply` `apply a82f --accept-risk --confirm` — runs `ono.change.apply`, whose verb `apply` changes the system (verbs.yaml)
// - `ono.change.verify` `verify a82f` — exempt: names the plan `a82f`, and a fresh state directory holds no plan.
// - `ono.change.recover` `recover a82f` — exempt: names the plan `a82f`, and a fresh state directory holds no plan.
// - `ono.change.recover` `recover a82f --goal restore-changed-objects` — exempt: names the plan `a82f`, and a fresh state directory holds no plan.
// - `ono.recovery.get` `get recovery --plan a82f` — exempt: names the plan `a82f`, and a fresh state directory holds no plan.
// - `ono.recovery.inspect` `inspect recovery r-a82f` — exempt: names the recovery asset `r-a82f`, and a fresh state directory holds none.
// - `ono.recovery.remove` `remove recovery r-a82f --dry-run` — runs `ono.recovery.remove`, whose verb `remove` changes the system (verbs.yaml)
// - `ono.recovery.remove` `remove recovery r-a82f --confirm` — runs `ono.recovery.remove`, whose verb `remove` changes the system (verbs.yaml)
// - `ono.container.get` `get container` — runs `ono.container.get`, which declares privilege `conditional`
// - `ono.container.get` `get container | where state == running` — runs `ono.container.get`, which declares privilege `conditional`
// - `ono.container.start` `start container web-1` — runs `ono.container.start`, whose verb `start` changes the system (verbs.yaml)
// - `ono.container.stop` `stop container web-1` — runs `ono.container.stop`, whose verb `stop` changes the system (verbs.yaml)
// - `ono.container.restart` `restart container web-1` — runs `ono.container.restart`, whose verb `restart` changes the system (verbs.yaml)
// - `ono.container.enter` `enter container web-1` — runs `ono.container.enter`, which declares privilege `conditional`
// - `ono.container.trace` `trace container web-1` — runs `ono.container.trace`, which declares privilege `conditional`
// - `ono.image.get` `get image` — runs `ono.image.get`, which declares privilege `conditional`
// - `ono.container.watch` `watch container` — runs `ono.container.watch`, whose verb `watch` follows a live source that does not end (verbs.yaml)
// - `ono.container.remove` `remove container web-1` — runs `ono.container.remove`, whose verb `remove` changes the system (verbs.yaml)
// - `ono.container.set` `set container web-1 --memory 2GiB` — runs `ono.container.set`, whose verb `set` changes the system (verbs.yaml)
// - `ono.data.where` `get socket | where state == established` — runs `ono.socket.get`, which declares privilege `conditional`
// - `ono.data.group` `get socket | where state == established | group process.name` — runs `ono.socket.get`, which declares privilege `conditional`
// - `ono.data.each` `get service | where state == failed | each { restart service @ }` — runs `ono.service.restart`, whose verb `restart` changes the system (verbs.yaml)
// - `ono.data.count` `echo (get process | count)` — runs `echo`, which no command contract declares — an external program or a shell keyword, whose effects nothing states
// - `ono.data.measure` `get process | measure memory` — runs `ono.data.measure`, which declares `ono.measure/1` — a schema schemas/deferred.yaml says a later phase writes
// - `ono.data.to` `get process | to json > out.json` — writes through a redirection
// - `ono.data.from` `curl -s https://example/api | from json | where status == "open"` — runs `curl`, which no command contract declares — an external program or a shell keyword, whose effects nothing states
// - `ono.data.view` `trace process 1 | view tree` — runs `ono.process.trace`, which declares privilege `conditional`
// - `ono.data.tail` `get log --service nginx | tail 30` — runs `ono.log.get`, which declares privilege `conditional`
// - `ono.data.diff` `get service | diff @-1` — refers to an earlier result, which the scratch shell does not have
// - `ono.file.get` `get file ./src` — runs `ono.file.get`, which declares privilege `conditional`
// - `ono.file.get` `get file /tmp --recursive | where modified < now()-30d` — runs `ono.file.get`, which declares privilege `conditional`
// - `ono.file.find` `find file /var/log` — runs `ono.file.find`, which declares privilege `conditional`
// - `ono.file.find` `find file /var/log | where size > 100MiB and modified < now()-30d` — runs `ono.file.find`, which declares privilege `conditional`
// - `ono.file.read` `read file /etc/hostname --encoding utf-8` — runs `ono.file.read`, which declares privilege `conditional`
// - `ono.file.read` `read file ./data.bin | to json` — runs `ono.file.read`, which declares privilege `conditional`
// - `ono.file.write` `get process | to json | write file ./processes.json --overwrite` — runs `ono.file.write`, whose verb `write` changes the system (verbs.yaml)
// - `ono.file.copy` `copy file ./a.txt ./b.txt` — runs `ono.file.copy`, whose verb `copy` changes the system (verbs.yaml)
// - `ono.file.move` `move file ./a.txt ./archive/a.txt` — runs `ono.file.move`, whose verb `move` changes the system (verbs.yaml)
// - `ono.file.remove` `remove file ./build.tmp` — runs `ono.file.remove`, whose verb `remove` changes the system (verbs.yaml)
// - `ono.file.remove` `find file /tmp | where modified < now()-30d | remove file` — runs `ono.file.find`, which declares privilege `conditional`
// - `ono.file.set` `set file ./script.sh --mode 0755` — runs `ono.file.set`, whose verb `set` changes the system (verbs.yaml)
// - `ono.file.trace` `trace file /var/lib/postgresql/data --users` — runs `ono.file.trace`, which declares privilege `conditional`
// - `ono.file.tail` `tail file /var/log/syslog --lines 50` — runs `ono.file.tail`, which declares privilege `conditional`
// - `ono.file.open` `open file ./report.pdf` — runs `ono.file.open`, whose verb `open` changes the system (verbs.yaml)
// - `ono.file.watch` `watch file ./src --recursive` — runs `ono.file.watch`, whose verb `watch` follows a live source that does not end (verbs.yaml)
// - `ono.file.enter` `enter file ./Cargo.toml` — runs `ono.file.enter`, which declares privilege `conditional`
// - `ono.dir.get` `get dir` — runs `ono.dir.get`, which declares privilege `conditional`
// - `ono.dir.get` `get dir ./src --recursive | where size > 1MiB` — runs `ono.dir.get`, which declares privilege `conditional`
// - `ono.dir.enter` `enter dir ./src` — runs `ono.dir.enter`, which declares privilege `conditional`
// - `ono.dir.remove` `remove dir ./build --recursive` — runs `ono.dir.remove`, whose verb `remove` changes the system (verbs.yaml)
// - `ono.dir.set` `set dir ./secrets --mode 0700` — runs `ono.dir.set`, whose verb `set` changes the system (verbs.yaml)
// - `ono.env.set` `set env RUST_LOG debug` — runs `ono.env.set`, whose verb `set` changes the system (verbs.yaml)
// - `ono.user.add` `add user deploy --shell /usr/bin/ono` — runs `ono.user.add`, whose verb `add` changes the system (verbs.yaml)
// - `ono.user.remove` `remove user deploy` — runs `ono.user.remove`, whose verb `remove` changes the system (verbs.yaml)
// - `ono.user.set` `set user deploy --shell /usr/bin/ono` — runs `ono.user.set`, whose verb `set` changes the system (verbs.yaml)
// - `ono.group.add` `add group docker --member deploy` — runs `ono.group.add`, whose verb `add` changes the system (verbs.yaml)
// - `ono.group.remove` `remove group docker --member deploy` — runs `ono.group.remove`, whose verb `remove` changes the system (verbs.yaml)
// - `ono.group.set` `set group docker --gid 999` — runs `ono.group.set`, whose verb `set` changes the system (verbs.yaml)
// - `ono.user.watch` `watch user` — runs `ono.user.watch`, whose verb `watch` follows a live source that does not end (verbs.yaml)
// - `ono.user.trace` `trace user postgres` — runs `ono.user.trace`, which declares privilege `conditional`
// - `ono.user.enter` `enter user postgres` — exempt: runs only as a statement of its own; in a pipeline this build implements nothing for it.
// - `ono.group.watch` `watch group` — runs `ono.group.watch`, whose verb `watch` follows a live source that does not end (verbs.yaml)
// - `ono.group.enter` `enter group docker` — exempt: runs only as a statement of its own; in a pipeline this build implements nothing for it.
// - `ono.plugin.inspect` `inspect plugin kubernetes` — exempt: names the package `kubernetes`, and a fresh plugin path has none installed.
// - `ono.plugin.inspect` `inspect plugin dev.example.packet-eye` — exempt: names the package `dev.example.packet-eye`, and a fresh plugin path has none installed.
// - `ono.plugin.install` `install plugin kubernetes` — runs `ono.plugin.install`, whose verb `install` changes the system (verbs.yaml)
// - `ono.plugin.install` `install plugin kubernetes --access recommended --confirm` — runs `ono.plugin.install`, whose verb `install` changes the system (verbs.yaml)
// - `ono.plugin.install` `install plugin kubernetes --source system --confirm` — runs `ono.plugin.install`, whose verb `install` changes the system (verbs.yaml)
// - `ono.plugin.install` `install plugin path:/srv/packages/dev.example.packet-eye --confirm` — runs `ono.plugin.install`, whose verb `install` changes the system (verbs.yaml)
// - `ono.plugin.remove` `remove plugin kubernetes` — runs `ono.plugin.remove`, whose verb `remove` changes the system (verbs.yaml)
// - `ono.plugin.remove` `remove plugin dev.example.packet-eye` — runs `ono.plugin.remove`, whose verb `remove` changes the system (verbs.yaml)
// - `ono.plugin.load` `load plugin dev.example.packet-eye` — runs `ono.plugin.load`, whose verb `load` changes the system (verbs.yaml)
// - `ono.plugin.unload` `unload plugin dev.example.packet-eye` — runs `ono.plugin.unload`, whose verb `unload` changes the system (verbs.yaml)
// - `ono.plugin.set` `set plugin dev.example.packet-eye --enabled false` — runs `ono.plugin.set`, whose verb `set` changes the system (verbs.yaml)
// - `ono.plugin.set` `set plugin dev.example.packet-eye --background true` — runs `ono.plugin.set`, whose verb `set` changes the system (verbs.yaml)
// - `ono.plugin.verify` `verify plugin dev.example.packet-eye` — exempt: names the package `dev.example.packet-eye`, and a fresh plugin path has none installed.
// - `ono.permission.get` `get permission kubernetes` — exempt: names the package `kubernetes`, and a fresh plugin path has none installed.
// - `ono.permission.get` `get permission kubernetes --all` — exempt: names the package `kubernetes`, and a fresh plugin path has none installed.
// - `ono.permission.set` `set permission kubernetes --profile operate` — runs `ono.permission.set`, whose verb `set` changes the system (verbs.yaml)
// - `ono.permission.set` `set permission kubernetes cluster-mutation --decision allow` — runs `ono.permission.set`, whose verb `set` changes the system (verbs.yaml)
// - `ono.permission.set` `set permission kubernetes credential-helper --decision allow --scope programs=/usr/bin/aws` — runs `ono.permission.set`, whose verb `set` changes the system (verbs.yaml)
// - `ono.capability.grant` `grant capability filesystem.read --plugin dev.example.packet-eye --duration 1h` — runs `ono.capability.grant`, whose verb `grant` changes the system (verbs.yaml)
// - `ono.capability.revoke` `revoke capability filesystem.read --plugin dev.example.packet-eye` — runs `ono.capability.revoke`, whose verb `revoke` changes the system (verbs.yaml)
// - `ono.assistant.ask` `ask assistant ops-assist "why did image-worker fail?"` — runs `ono.assistant.ask`, whose capability `assistant.ask` is `mutate` rather than `read`
// - `ono.meta.help` `help` — runs `ono.meta.help`, which declares `ono.help-page/1` — a schema schemas/deferred.yaml says a later phase writes
// - `ono.meta.help` `help get process` — runs `ono.meta.help`, which declares `ono.help-page/1` — a schema schemas/deferred.yaml says a later phase writes
// - `ono.meta.type` `get process | type` — runs `ono.meta.type`, which declares `ono.type-info/1` — a schema schemas/deferred.yaml says a later phase writes
// - `ono.meta.type` `type get socket` — runs `ono.meta.type`, which declares `ono.type-info/1` — a schema schemas/deferred.yaml says a later phase writes
// - `ono.meta.inspect` `inspect @1` — runs `ono.meta.inspect`, which declares `ono.inspection/1` — a schema schemas/deferred.yaml says a later phase writes
// - `ono.meta.inspect` `get service | where state == failed | take 1 | inspect` — runs `ono.meta.inspect`, which declares `ono.inspection/1` — a schema schemas/deferred.yaml says a later phase writes
// - `ono.meta.explain` `explain get process` — runs `ono.meta.explain`, which declares `ono.execution-plan/1` — a schema schemas/deferred.yaml says a later phase writes
// - `ono.meta.explain` `explain get file /tmp --recursive | remove file` — runs `ono.meta.explain`, which declares `ono.execution-plan/1` — a schema schemas/deferred.yaml says a later phase writes
// - `ono.config.set` `set config prompt.path = "smart"` — runs `ono.config.set`, whose verb `set` changes the system (verbs.yaml)
// - `ono.config.set` `set config render.table.max_rows = 200` — runs `ono.config.set`, whose verb `set` changes the system (verbs.yaml)
// - `ono.config.set` `set config limits.history_bytes_total = 64MiB` — runs `ono.config.set`, whose verb `set` changes the system (verbs.yaml)
// - `ono.config.set` `set config safety.confirm.bulk_threshold = 100` — runs `ono.config.set`, whose verb `set` changes the system (verbs.yaml)
// - `ono.context.leave` `leave` — runs `leave`, which no command contract declares — an external program or a shell keyword, whose effects nothing states
// - `ono.context.leave` `leave --all` — runs `leave`, which no command contract declares — an external program or a shell keyword, whose effects nothing states
// - `ono.socket.get` `get socket` — runs `ono.socket.get`, which declares privilege `conditional`
// - `ono.socket.get` `get socket | where state == established | group process.name` — runs `ono.socket.get`, which declares privilege `conditional`
// - `ono.socket.get` `get socket | where state == listen | where local.address not in [127.0.0.1, ::1]` — runs `ono.socket.get`, which declares privilege `conditional`
// - `ono.connection.get` `get connection` — runs `ono.connection.get`, which declares privilege `conditional`
// - `ono.connection.get` `get connection --remote 10.4.2.11` — runs `ono.connection.get`, which declares privilege `conditional`
// - `ono.dns.resolve` `resolve dns example.com` — runs `ono.dns.resolve`, whose capability `dns.resolve` reaches the network
// - `ono.dns.resolve` `resolve dns 10.4.2.11` — runs `ono.dns.resolve`, whose capability `dns.resolve` reaches the network
// - `ono.dns.resolve` `resolve dns example.com --server 9.9.9.9` — runs `ono.dns.resolve`, whose capability `dns.resolve` reaches the network
// - `ono.port.test` `test port example.com 443` — runs `ono.port.test`, whose capability `port.probe` is `observe` rather than `read`
// - `ono.port.test` `test port 10.4.2.11 5432 --timeout 2s` — runs `ono.port.test`, whose capability `port.probe` is `observe` rather than `read`
// - `ono.connection.trace` `trace connection --remote 10.4.2.11` — runs `ono.connection.trace`, which declares privilege `conditional`
// - `ono.socket.trace` `trace socket --port 443` — runs `ono.socket.trace`, which declares privilege `conditional`
// - `ono.socket.watch` `watch socket 443` — runs `ono.socket.watch`, whose verb `watch` follows a live source that does not end (verbs.yaml)
// - `ono.interface.watch` `watch interface` — runs `ono.interface.watch`, whose verb `watch` follows a live source that does not end (verbs.yaml)
// - `ono.route.watch` `watch route` — runs `ono.route.watch`, whose verb `watch` follows a live source that does not end (verbs.yaml)
// - `ono.route.add` `add route 10.0.0.0/8 --gateway 192.168.1.1` — runs `ono.route.add`, whose verb `add` changes the system (verbs.yaml)
// - `ono.route.remove` `remove route 10.0.0.0/8` — runs `ono.route.remove`, whose verb `remove` changes the system (verbs.yaml)
// - `ono.route.set` `set route 10.0.0.0/8 --metric 200` — runs `ono.route.set`, whose verb `set` changes the system (verbs.yaml)
// - `ono.interface.set` `set interface eth0 --mtu 9000` — runs `ono.interface.set`, whose verb `set` changes the system (verbs.yaml)
// - `ono.interface.start` `start interface eth0` — runs `ono.interface.start`, whose verb `start` changes the system (verbs.yaml)
// - `ono.interface.stop` `stop interface eth0` — runs `ono.interface.stop`, whose verb `stop` changes the system (verbs.yaml)
// - `ono.interface.add` `add interface eth0 --address 192.168.1.5/24` — runs `ono.interface.add`, whose verb `add` changes the system (verbs.yaml)
// - `ono.interface.remove` `remove interface eth0 --address 192.168.1.5/24` — runs `ono.interface.remove`, whose verb `remove` changes the system (verbs.yaml)
// - `ono.interface.trace` `trace interface eth0` — runs `ono.interface.trace`, which declares privilege `conditional`
// - `ono.interface.enter` `enter interface eth0` — exempt: runs only as a statement of its own; in a pipeline this build implements nothing for it.
// - `ono.socket.stop` `get socket | where remote.address == 10.4.2.11 | stop socket` — runs `ono.socket.get`, which declares privilege `conditional`
// - `ono.socket.enter` `get socket | take 1 | enter socket` — runs `ono.socket.get`, which declares privilege `conditional`
// - `ono.socket.enter` `enter socket 443` — runs `ono.socket.enter`, which declares privilege `conditional`
// - `ono.package.add` `add package nginx` — runs `ono.package.add`, whose verb `add` changes the system (verbs.yaml)
// - `ono.package.remove` `remove package nginx` — runs `ono.package.remove`, whose verb `remove` changes the system (verbs.yaml)
// - `ono.package.set` `set package nginx --version 1.24.0` — runs `ono.package.set`, whose verb `set` changes the system (verbs.yaml)
// - `ono.package-source.refresh` `refresh package-source updates` — runs `ono.package-source.refresh`, whose verb `refresh` changes the system (verbs.yaml)
// - `ono.process.get` `get process 4419` — exempt: names the process 4419, which a test host need not be running.
// - `ono.process.inspect` `inspect process 4419` — runs `ono.process.inspect`, which declares privilege `conditional`
// - `ono.process.inspect` `get process | where memory > 1GiB | take 1 | inspect process` — runs `ono.process.inspect`, which declares privilege `conditional`
// - `ono.process.watch` `watch process --every 1s | where cpu > 20` — runs `ono.process.watch`, whose verb `watch` follows a live source that does not end (verbs.yaml)
// - `ono.process.watch` `watch process --service nginx &` — runs `ono.process.watch`, whose verb `watch` follows a live source that does not end (verbs.yaml)
// - `ono.process.stop` `stop process 4419` — runs `ono.process.stop`, whose verb `stop` changes the system (verbs.yaml)
// - `ono.process.stop` `get process | where name == "foo" | stop process | where status == failed` — runs `ono.process.stop`, whose verb `stop` changes the system (verbs.yaml)
// - `ono.process.kill` `kill process 4419` — runs `ono.process.kill`, whose verb `kill` changes the system (verbs.yaml)
// - `ono.process.kill` `kill process 4419 --signal SIGHUP` — runs `ono.process.kill`, whose verb `kill` changes the system (verbs.yaml)
// - `ono.process.trace` `trace process 812` — runs `ono.process.trace`, which declares privilege `conditional`
// - `ono.process.trace` `trace @1` — runs `trace`, which no command contract declares — an external program or a shell keyword, whose effects nothing states
// - `ono.process.enter` `enter process 4419` — exempt: runs only as a statement of its own; in a pipeline this build implements nothing for it.
// - `ono.process.set` `set process 4419 --priority 10` — runs `ono.process.set`, whose verb `set` changes the system (verbs.yaml)
// - `ono.signal.send` `get process 4419 | send signal SIGHUP` — runs `ono.signal.send`, whose verb `send` changes the system (verbs.yaml)
// - `ono.host.link` `link host prod-db` — runs `ono.host.link`, whose verb `link` changes the system (verbs.yaml)
// - `ono.host.link` `link host prod-web-3` — runs `ono.host.link`, whose verb `link` changes the system (verbs.yaml)
// - `ono.link.detach` `detach link prod-db` — runs `ono.link.detach`, whose capability `link.manage` is `mutate` rather than `read`
// - `ono.link.enter` `enter link prod-db` — exempt: runs only as a statement of its own; in a pipeline this build implements nothing for it.
// - `ono.host.test` `test host prod-db` — runs `ono.host.test`, whose capability `host.probe` is `observe` rather than `read`
// - `ono.host.connect` `connect host prod-db` — runs `ono.host.connect`, whose verb `connect` changes the system (verbs.yaml)
// - `ono.host.trace` `trace host prod-db` — exempt: names the host `prod-db`, which a fresh configuration does not know.
// - `ono.link.watch` `watch link prod-db` — runs `ono.link.watch`, whose verb `watch` follows a live source that does not end (verbs.yaml)
// - `ono.link.add` `add link prod-db --host 10.4.2.11` — runs `ono.link.add`, whose verb `add` changes the system (verbs.yaml)
// - `ono.link.remove` `remove link prod-db` — runs `ono.link.remove`, whose verb `remove` changes the system (verbs.yaml)
// - `ono.link.set` `set link prod-db --transport ssh` — runs `ono.link.set`, whose verb `set` changes the system (verbs.yaml)
// - `ono.link.rename` `rename link prod-db prod-db-primary` — runs `ono.link.rename`, whose verb `rename` changes the system (verbs.yaml)
// - `ono.host.watch` `watch host prod-db --every 30s` — runs `ono.host.watch`, whose verb `watch` follows a live source that does not end (verbs.yaml)
// - `ono.host.add` `add host prod-db --address 10.4.2.11` — runs `ono.host.add`, whose verb `add` changes the system (verbs.yaml)
// - `ono.host.remove` `remove host prod-db` — runs `ono.host.remove`, whose verb `remove` changes the system (verbs.yaml)
// - `ono.host.set` `set host prod-db --address 10.4.2.12` — runs `ono.host.set`, whose verb `set` changes the system (verbs.yaml)
// - `ono.link.trace` `trace link prod-db` — exempt: names the link `prod-db`, which a fresh configuration does not hold.
// - `ono.host-key.add` `add host-key prod-db --fingerprint sha256:1f0c` — runs `ono.host-key.add`, whose verb `add` changes the system (verbs.yaml)
// - `ono.host-key.set` `set host-key prod-db --fingerprint sha256:9ab4` — runs `ono.host-key.set`, whose verb `set` changes the system (verbs.yaml)
// - `ono.host-key.remove` `remove host-key prod-db` — runs `ono.host-key.remove`, whose verb `remove` changes the system (verbs.yaml)
// - `ono.client-key.add` `add client-key sha256:1f0c --label deploy` — runs `ono.client-key.add`, whose verb `add` changes the system (verbs.yaml)
// - `ono.client-key.set` `set client-key sha256:1f0c --allow service.manage` — runs `ono.client-key.set`, whose verb `set` changes the system (verbs.yaml)
// - `ono.client-key.set` `set client-key sha256:1f0c --observe false` — runs `ono.client-key.set`, whose verb `set` changes the system (verbs.yaml)
// - `ono.client-key.remove` `remove client-key sha256:1f0c` — runs `ono.client-key.remove`, whose verb `remove` changes the system (verbs.yaml)
// - `ono.service.start` `start service nginx` — runs `ono.service.start`, whose verb `start` changes the system (verbs.yaml)
// - `ono.service.stop` `stop service nginx` — runs `ono.service.stop`, whose verb `stop` changes the system (verbs.yaml)
// - `ono.service.restart` `restart service nginx` — runs `ono.service.restart`, whose verb `restart` changes the system (verbs.yaml)
// - `ono.service.restart` `get service | where state == failed | restart service` — runs `ono.service.restart`, whose verb `restart` changes the system (verbs.yaml)
// - `ono.service.watch` `watch service nginx` — runs `ono.service.watch`, whose verb `watch` follows a live source that does not end (verbs.yaml)
// - `ono.service.watch` `watch service nginx &` — runs `ono.service.watch`, whose verb `watch` follows a live source that does not end (verbs.yaml)
// - `ono.service.trace` `trace service nginx` — runs `ono.service.trace`, which declares privilege `conditional`
// - `ono.service.enter` `enter service nginx` — exempt: runs only as a statement of its own; in a pipeline this build implements nothing for it.
// - `ono.service.set` `set service nginx --enabled true` — runs `ono.service.set`, whose verb `set` changes the system (verbs.yaml)
// - `ono.log.get` `get log --service nginx | tail 30` — runs `ono.log.get`, which declares privilege `conditional`
// - `ono.log.get` `get log --service @1 | where level >= error | take 20` — runs `ono.log.get`, which declares privilege `conditional`
// - `ono.log.get` `get log --level error | take 20` — runs `ono.log.get`, which declares privilege `conditional`
// - `ono.journal.get` `get journal --since (now() - 1h)` — runs `ono.journal.get`, which declares privilege `conditional`
// - `ono.journal.get` `get journal --boot 0 | where priority <= 3` — runs `ono.journal.get`, which declares privilege `conditional`
// - `ono.journal.tail` `tail journal --lines 50` — runs `ono.journal.tail`, which declares privilege `conditional`
// - `ono.journal.tail` `tail journal | where priority <= 3 | take 1` — runs `ono.journal.tail`, which declares privilege `conditional`
// - `ono.place.find` `find place nginx` — runs `ono.place.find`, which declares privilege `conditional`
// - `ono.place.find` `find place --type service --where state == "active"` — runs `ono.place.find`, which declares privilege `conditional`
// - `ono.place.find` `find place --where local.port == 8080 | take 1` — runs `ono.place.find`, which declares privilege `conditional`
// - `ono.place.find` `find place --role workload` — runs `ono.place.find`, which declares privilege `conditional`
// - `ono.place.near` `near` — runs `ono.place.near`, which declares privilege `conditional`
// - `ono.place.near` `near --limit 5` — runs `ono.place.near`, which declares privilege `conditional`
// - `ono.place.near` `near --type process | take 3` — runs `ono.place.near`, which declares privilege `conditional`
// - `ono.place.enter` `enter compute` — runs `ono.place.enter`, which declares privilege `conditional`
// - `ono.place.enter` `enter processes` — runs `ono.place.enter`, which declares privilege `conditional`
// - `ono.place.enter` `enter network` — runs `ono.place.enter`, which declares privilege `conditional`
// - `ono.place.follow` `follow parent` — runs `ono.place.follow`, which declares privilege `conditional`
// - `ono.place.follow` `follow socket :443` — runs `ono.place.follow`, which declares privilege `conditional`
// - `ono.place.follow` `follow service` — runs `ono.place.follow`, which declares privilege `conditional`
// - `ono.place.map` `map` — runs `ono.place.map`, which declares privilege `conditional`
// - `ono.place.map` `map --json` — runs `ono.place.map`, which declares privilege `conditional`
// - `ono.place.map` `map --zoom 1` — runs `ono.place.map`, which declares privilege `conditional`
// - `ono.place.map` `map --all --type process` — runs `ono.place.map`, which declares privilege `conditional`
// - `ono.place.map` `map --live --json` — runs `ono.place.map`, which declares privilege `conditional`
// - `ono.place.back` `back` — exempt: returns to the previous place, and a fresh session has not moved (spatial.history_empty).
// - `ono.place.back` `follow socket :443; back` — runs `ono.place.follow`, which declares privilege `conditional`
// - `ono.place.up` `up` — exempt: moves to the parent place, and a fresh one-shot session starts at the top of the host's canonical hierarchy (spatial.no_parent).
// - `ono.place.up` `home; enter compute; enter processes; up` — runs `ono.place.enter`, which declares privilege `conditional`
// - `ono.place.jump` `jump storage:/data` — runs `ono.place.jump`, which declares privilege `conditional`
// - `ono.place.jump` `jump process/1842` — runs `ono.place.jump`, which declares privilege `conditional`
// - `ono.place.jump` `jump @edge-proxy` — runs `ono.place.jump`, which declares privilege `conditional`
// - `ono.place.unpin` `unpin` — exempt: removes the pin of the current place, and a fresh session has pinned nothing.
// - `ono.place.unpin` `unpin edge-proxy` — exempt: removes the pin `edge-proxy`, and a fresh session has pinned nothing.
// - `ono.filesystem.get` `get filesystem` — runs `ono.filesystem.get`, which declares privilege `conditional`
// - `ono.filesystem.get` `get filesystem | where available < 1GiB` — runs `ono.filesystem.get`, which declares privilege `conditional`
// - `ono.filesystem.mount` `mount filesystem /dev/sdb1 /mnt/data --read-only` — runs `ono.filesystem.mount`, whose verb `mount` changes the system (verbs.yaml)
// - `ono.filesystem.unmount` `unmount filesystem /mnt/data` — runs `ono.filesystem.unmount`, whose verb `unmount` changes the system (verbs.yaml)
// - `ono.mount.set` `set mount /mnt/data --read-only false` — runs `ono.mount.set`, whose verb `set` changes the system (verbs.yaml)
// - `ono.mount.add` `add mount /dev/sdb1 /mnt/data --type ext4` — runs `ono.mount.add`, whose verb `add` changes the system (verbs.yaml)
// - `ono.mount.remove` `remove mount /mnt/data` — runs `ono.mount.remove`, whose verb `remove` changes the system (verbs.yaml)
// - `ono.mount.start` `start mount /mnt/data` — runs `ono.mount.start`, whose verb `start` changes the system (verbs.yaml)
// - `ono.mount.stop` `stop mount /mnt/data` — runs `ono.mount.stop`, whose verb `stop` changes the system (verbs.yaml)
// - `ono.mount.watch` `watch mount` — runs `ono.mount.watch`, whose verb `watch` follows a live source that does not end (verbs.yaml)
// - `ono.mount.trace` `trace mount /mnt/data` — runs `ono.mount.trace`, which declares privilege `conditional`
// - `ono.mount.enter` `enter mount /mnt/data` — exempt: runs only as a statement of its own; in a pipeline this build implements nothing for it.
// - `ono.temporal.at` `at -10m` — exempt: asks for recorded history, and a fresh shell has recorded nothing: recording is off until `start recorder` (v0.5 §10.2), which mutates.
// - `ono.temporal.at` `at 12:17` — exempt: asks for recorded history, and a fresh shell has recorded nothing: recording is off until `start recorder` (v0.5 §10.2), which mutates.
// - `ono.temporal.at` `at 2026-08-31T12:17:00+02:00` — exempt: asks for recorded history, and a fresh shell has recorded nothing: recording is off until `start recorder` (v0.5 §10.2), which mutates.
// - `ono.temporal.at` `at event @e42` — refers to `@e42`, a result or event of a session the scratch shell does not have
// - `ono.temporal.present` `present git status` — exempt: runs `git status`, an external program whose effects and output no contract states, in a scratch directory that is not a repository.
// - `ono.temporal.present` `present printf ok` — exempt: runs `printf`, an external program that writes to the terminal before the action result, so its output is not one inspectable document.
// - `ono.temporal.why` `why event @e42` — refers to `@e42`, a result or event of a session the scratch shell does not have
// - `ono.event.inspect` `inspect event @e42` — refers to `@e42`, a result or event of a session the scratch shell does not have
// - `ono.recorder.start` `start recorder` — runs `ono.recorder.start`, whose verb `start` changes the system (verbs.yaml)
// - `ono.recorder.stop` `stop recorder` — runs `ono.recorder.stop`, whose verb `stop` changes the system (verbs.yaml)
// - `ono.temporal-history.remove` `remove temporal-history --confirm` — runs `ono.temporal-history.remove`, whose verb `remove` changes the system (verbs.yaml)
// - `ono.temporal-history.remove` `remove temporal-history --dry-run` — runs `ono.temporal-history.remove`, whose verb `remove` changes the system (verbs.yaml)
