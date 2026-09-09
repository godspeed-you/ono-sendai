//! The historical-context guard (spec v0.6 §6.4, §2.5).
//!
//! §2.5 is one of the core invariants: *"Historical context remains read-only. A change plan that
//! will be applied MUST be resolved against present state."* §6.4 gives it its own refusal, with
//! its own sentence, because the generic read-only error would not say the thing an operator at
//! `local:// @12:17 [PAST]` needs to hear — that history can be inspected but not used as the
//! executable target base, and that `now` is the way back.
//!
//! The rule is a function rather than a check inside the builder because the CLI knows the
//! session's temporal coordinate and the builder does not. §6.4 is about where the operator is
//! standing, not about what the plan contains.

use ono_change_core::error;
use ono_value::ErrorValue;

/// Refuses `command` when the session is standing in the past (§6.4).
///
/// `is_historical` is a parameter rather than something read from a session, so the rule is
/// testable and the CLI keeps the one place that knows where the cursor is.
///
/// # Errors
///
/// Returns `change.historical_context_read_only` when `is_historical` is true.
pub fn refuse_in_past(command: &str, is_historical: bool) -> Result<(), ErrorValue> {
    if is_historical {
        return Err(error::historical_context_read_only(command));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::unwrap_used,
        clippy::panic,
        reason = "a test states its preconditions directly (AGENTS.md section 16)"
    )]

    use ono_core::ErrorCode;
    use ono_value::Value;

    use super::*;

    #[test]
    fn should_refuse_to_plan_from_a_historical_coordinate() {
        let refusal = refuse_in_past("plan restart service nginx", true)
            .expect_err("§6.4 refuses a plan resolved against the past");
        assert_eq!(
            refusal.code(),
            ErrorCode::ChangeHistoricalContextReadOnly,
            "§6.4: historical state can be inspected but not used as the executable target base"
        );
    }

    #[test]
    fn should_let_a_plan_through_in_the_present() {
        assert!(
            refuse_in_past("plan restart service nginx", false).is_ok(),
            "§6.4 is about where the session is standing, and the present is where plans resolve"
        );
    }

    #[test]
    fn should_name_the_command_it_refused_so_a_script_can_match_on_it() {
        let refusal = refuse_in_past("plan restart service nginx", true)
            .expect_err("§6.4 refuses a plan resolved against the past");
        assert_eq!(
            refusal.metadata().get("command"),
            Some(&Value::string("plan restart service nginx")),
            "§45: a refusal is a structured value a script can branch on"
        );
    }

    #[test]
    fn should_explain_that_now_returns_to_the_present() {
        let refusal = refuse_in_past("plan restart service nginx", true)
            .expect_err("§6.4 refuses a plan resolved against the past");
        let help = refusal.help().unwrap_or_default();
        assert!(
            help.contains("§6.4") && help.contains("now"),
            "§6.4 wants its own sentence, not the generic read-only one; got `{help}`"
        );
    }

    #[test]
    fn should_refuse_every_command_the_caller_hands_it_while_in_the_past() {
        for command in [
            "plan restart service nginx",
            "apply plan/a82f",
            "recover plan/a82f",
        ] {
            assert!(
                refuse_in_past(command, true).is_err(),
                "§2.5: a change plan that will be applied MUST be resolved against present state, \
                 and `{command}` would not be"
            );
        }
    }
}
