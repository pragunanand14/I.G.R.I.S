//! Recovery: what to do when an action fails.
//!
//! failed action → classify → retry if safe → otherwise let the model choose
//! another approach → otherwise fail honestly. Bounded three ways: only
//! read-only calls are retried automatically (once), the same failing call
//! isn't repeated more than [`MAX_SAME_FAILURES`] times, and a task stops
//! after [`MAX_TASK_FAILURES`] failed actions.

use std::collections::HashMap;

use crate::ai::ToolCall;
use crate::tools::executor::{ActivityStatus, ToolActivity};
use crate::tools::{PermissionLevel, ToolErrorKind};

/// Failed actions a task may accumulate before it stops.
pub const MAX_TASK_FAILURES: u32 = 5;
/// Times the exact same call may fail before it's refused.
pub const MAX_SAME_FAILURES: usize = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failure {
    /// The user said no (or didn't answer).
    Denied,
    Cancelled,
    /// The model sent bad input or called something that doesn't exist.
    Invalid,
    /// Timed out or a transient error; trying again may work.
    Transient,
    /// Deliberately not done (a safety check, a changed screen) or not found.
    Refused,
    /// The action ran but verification showed it didn't have the intended effect.
    Unconfirmed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// Run the same call again now (read-only calls only).
    RetryNow,
    /// Report the failure to the model so it can adjust its approach.
    Adjust,
    /// The task can't continue.
    Stop { cancelled: bool, reason: String },
}

/// Why a call failed, from the executor's outcome. `None` if it succeeded.
pub fn classify(activity: &ToolActivity) -> Option<Failure> {
    match activity.status {
        ActivityStatus::Completed | ActivityStatus::Running | ActivityStatus::AwaitingApproval => None,
        ActivityStatus::Denied => Some(Failure::Denied),
        ActivityStatus::Cancelled => Some(Failure::Cancelled),
        ActivityStatus::Invalid => Some(Failure::Invalid),
        ActivityStatus::Failed => Some(match activity.failure {
            Some(ToolErrorKind::Refused | ToolErrorKind::NotFound) => Failure::Refused,
            _ => Failure::Transient,
        }),
    }
}

/// What to do about a failure. `attempt` counts automatic retries of this
/// call so far; `failures` is the task's failure count including this one.
pub fn decide(failure: Failure, permission: Option<PermissionLevel>, attempt: u32, failures: u32, description: &str) -> Decision {
    match failure {
        Failure::Cancelled => Decision::Stop { cancelled: true, reason: "Stopped by the user.".into() },
        Failure::Denied => Decision::Stop { cancelled: false, reason: format!("Not approved: {description}.") },
        // Only calls without side effects are retried automatically: repeating
        // an action could do it twice.
        Failure::Transient if permission == Some(PermissionLevel::Safe) && attempt == 0 => Decision::RetryNow,
        _ if failures >= MAX_TASK_FAILURES => {
            Decision::Stop { cancelled: false, reason: format!("{failures} actions failed (last: {description}); stopping instead of trying again.") }
        }
        _ => Decision::Adjust,
    }
}

/// Guidance appended to a failed result for the model.
pub fn guidance(failure: Failure, failures: u32) -> String {
    let left = MAX_TASK_FAILURES.saturating_sub(failures);
    let what = match failure {
        Failure::Unconfirmed => "The action reported success, but the check shows it didn't have the intended effect.",
        Failure::Invalid => "Fix the input before trying again.",
        Failure::Refused => "Read why it was refused and change the approach; don't repeat the same call.",
        Failure::Transient => "Check the current state before trying again, or try a different approach.",
        Failure::Denied | Failure::Cancelled => "Don't retry it.",
    };
    format!("[IGRIS task: {what} {left} more failed action(s) allowed before the task stops.]")
}

fn key(call: &ToolCall) -> (String, String) {
    (call.name.clone(), call.invalid_input.clone().unwrap_or_else(|| call.input.to_string()))
}

/// Stops the model from repeating an identical failing call.
#[derive(Debug, Default)]
pub struct RepeatGuard {
    failed: HashMap<(String, String), usize>,
}

impl RepeatGuard {
    /// This exact call has already failed too often.
    pub fn blocked(&self, call: &ToolCall) -> bool {
        self.failed.get(&key(call)).is_some_and(|n| *n >= MAX_SAME_FAILURES)
    }
    pub fn failed(&mut self, call: &ToolCall) {
        *self.failed.entry(key(call)).or_default() += 1;
    }
    pub fn succeeded(&mut self, call: &ToolCall) {
        self.failed.remove(&key(call));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn activity(status: ActivityStatus, failure: Option<ToolErrorKind>) -> ToolActivity {
        ToolActivity {
            id: "a".into(),
            tool: "t".into(),
            title: "t".into(),
            permission: Some(PermissionLevel::Low),
            description: "d".into(),
            status,
            result: None,
            duration_ms: None,
            text_offset: None,
            sources: vec![],
            attachments: vec![],
            failure,
            input_digest: None,
        }
    }

    #[test]
    fn failures_are_classified_from_the_executor_outcome() {
        assert_eq!(classify(&activity(ActivityStatus::Completed, None)), None);
        assert_eq!(classify(&activity(ActivityStatus::Denied, None)), Some(Failure::Denied));
        assert_eq!(classify(&activity(ActivityStatus::Cancelled, None)), Some(Failure::Cancelled));
        assert_eq!(classify(&activity(ActivityStatus::Invalid, None)), Some(Failure::Invalid));
        assert_eq!(classify(&activity(ActivityStatus::Failed, Some(ToolErrorKind::Timeout))), Some(Failure::Transient));
        assert_eq!(classify(&activity(ActivityStatus::Failed, Some(ToolErrorKind::Refused))), Some(Failure::Refused));
        assert_eq!(classify(&activity(ActivityStatus::Failed, Some(ToolErrorKind::NotFound))), Some(Failure::Refused));
    }

    #[test]
    fn only_safe_calls_are_retried_and_only_once() {
        assert_eq!(decide(Failure::Transient, Some(PermissionLevel::Safe), 0, 1, "x"), Decision::RetryNow);
        assert_eq!(decide(Failure::Transient, Some(PermissionLevel::Safe), 1, 1, "x"), Decision::Adjust, "second failure goes to the model");
        // An action with side effects is never repeated automatically.
        assert_eq!(decide(Failure::Transient, Some(PermissionLevel::Low), 0, 1, "x"), Decision::Adjust);
        assert_eq!(decide(Failure::Transient, Some(PermissionLevel::Sensitive), 0, 1, "x"), Decision::Adjust);
        assert_eq!(decide(Failure::Refused, Some(PermissionLevel::Safe), 0, 1, "x"), Decision::Adjust);
    }

    #[test]
    fn the_failure_budget_and_the_user_end_the_task() {
        assert!(matches!(decide(Failure::Invalid, None, 0, MAX_TASK_FAILURES, "x"), Decision::Stop { cancelled: false, .. }));
        assert_eq!(decide(Failure::Invalid, None, 0, MAX_TASK_FAILURES - 1, "x"), Decision::Adjust);
        assert!(
            matches!(decide(Failure::Denied, Some(PermissionLevel::Sensitive), 0, 1, "Overwrite a"), Decision::Stop { cancelled: false, reason } if reason.contains("Overwrite a"))
        );
        assert!(matches!(decide(Failure::Cancelled, None, 0, 1, "x"), Decision::Stop { cancelled: true, .. }));
        assert!(guidance(Failure::Transient, 2).contains("3 more"));
    }

    #[test]
    fn identical_failing_calls_are_blocked_after_two_attempts() {
        let c = ToolCall { id: "1".into(), name: "write_file".into(), input: json!({"path": "a"}), invalid_input: None, extras: None };
        let other = ToolCall { input: json!({"path": "b"}), ..c.clone() };
        let mut g = RepeatGuard::default();
        g.failed(&c);
        assert!(!g.blocked(&c));
        g.failed(&c);
        assert!(g.blocked(&c));
        assert!(!g.blocked(&other), "a different call is fine");
        g.succeeded(&c);
        assert!(!g.blocked(&c));
    }
}
