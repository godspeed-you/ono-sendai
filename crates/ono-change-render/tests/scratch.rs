//! Scratch dump used while shaping the views.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "scratch"
)]
mod support;
use ono_change_render::*;
#[test]
fn dump() {
    for line in plan_view(
        &support::sealed_nginx_plan(),
        &[support::zfs_asset()],
        80,
        Charset::Ascii,
    ) {
        println!("{line}");
    }
    println!("==== collapsed");
    for line in collapsed_plan(&support::long_plan(), 80, Charset::Ascii) {
        println!("{line}");
    }
    println!("==== recovery");
    for line in recovery_view(&support::selective_recovery(), 80, Charset::Ascii) {
        println!("{line}");
    }
    println!("==== rollback");
    for line in recovery_view(&support::rollback_recovery(), 80, Charset::Ascii) {
        println!("{line}");
    }
    println!("==== unanalysed");
    for line in recovery_view(&support::unanalysed_recovery(), 80, Charset::Ascii) {
        println!("{line}");
    }
    println!("==== assets");
    for line in recovery_assets(
        &[support::ready_asset(), support::unmeasured_asset()],
        support::later(840),
        80,
    ) {
        println!("{line}");
    }
    println!("==== asset block");
    for line in recovery_asset_block(&support::zfs_asset(), 80, Charset::Ascii) {
        println!("{line}");
    }
    println!("==== matrix");
    for line in coverage_matrix(&support::protected_summary(), 80, Charset::Ascii) {
        println!("{line}");
    }
    println!("==== impact");
    for line in impact_block(&support::nginx_impact(), 80, Charset::Ascii) {
        println!("{line}");
    }
    println!("==== failure");
    for line in apply_failure(
        &support::failed_plan(),
        &[support::ready_asset()],
        80,
        Charset::Ascii,
    ) {
        println!("{line}");
    }
    println!("==== progress");
    let p = support::failed_plan();
    for line in apply_progress(&p, &[], 80) {
        println!("{line}");
    }
    println!("==== verify");
    let plan = support::sealed_nginx_plan();
    for line in verification_view(plan.id(), &support::nginx_results(&plan), 80) {
        println!("{line}");
    }
    println!("==== keys");
    for line in key_help(80) {
        println!("{line}");
    }
}
