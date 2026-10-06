//! `task_plan`: the model declares or revises the plan of the current task.
//!
//! Calling it makes the turn a task (see `orchestrator::intent`). The plan is
//! the model's working hypothesis; completion is still decided by the
//! orchestrator from verified results, not by steps the model marks done.

use std::sync::Arc;

use serde_json::{json, Value};

use super::{PermissionLevel, Tool, ToolCtx, ToolError, ToolOutput, ToolResultT, ToolSpec};
use crate::orchestrator::task::{PlanStep, StepStatus};
use crate::orchestrator::{Orchestrator, TaskEvent};

pub const MAX_STEPS: usize = 12;

pub struct TaskPlanTool {
    spec: ToolSpec,
    hub: Arc<Orchestrator>,
}

impl TaskPlanTool {
    pub fn new(hub: Arc<Orchestrator>) -> Self {
        Self {
            spec: ToolSpec {
                name: "task_plan",
                title: "Task plan",
                description: "For requests that take several actions (build something, research and write it up, fix failing tests, \
a multi-step job on the computer): record your plan as short steps before you start, and send the updated plan again when \
a step is done or when what you observe changes the plan. Statuses: pending, active, completed, failed, skipped. Don't use \
it for simple questions or single actions. The user sees the plan and its progress.",
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "steps": {
                            "type": "array",
                            "minItems": 1,
                            "maxItems": MAX_STEPS,
                            "items": {
                                "type": "object",
                                "properties": {
                                    "title": { "type": "string", "minLength": 1, "maxLength": 120 },
                                    "status": { "type": "string", "enum": ["pending", "active", "completed", "failed", "skipped"] }
                                },
                                "required": ["title", "status"],
                                "additionalProperties": false
                            }
                        }
                    },
                    "required": ["steps"],
                    "additionalProperties": false
                }),
                permission: PermissionLevel::Safe,
            },
            hub,
        }
    }
}

fn steps(input: &Value) -> Vec<PlanStep> {
    input["steps"]
        .as_array()
        .into_iter()
        .flatten()
        .take(MAX_STEPS)
        .map(|s| PlanStep {
            title: s["title"].as_str().unwrap_or_default().trim().to_string(),
            status: match s["status"].as_str() {
                Some("active") => StepStatus::Active,
                Some("completed") => StepStatus::Completed,
                Some("failed") => StepStatus::Failed,
                Some("skipped") => StepStatus::Skipped,
                _ => StepStatus::Pending,
            },
        })
        .collect()
}

#[async_trait::async_trait]
impl Tool for TaskPlanTool {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }
    fn describe(&self, input: &Value) -> String {
        format!("Plan: {} step(s)", input["steps"].as_array().map(Vec::len).unwrap_or(0))
    }
    async fn execute(&self, _input: &Value) -> ToolResultT {
        Err(ToolError::failed("task_plan needs a task."))
    }
    async fn execute_in(&self, input: &Value, ctx: &ToolCtx) -> ToolResultT {
        let id = ctx.task_id.as_deref().ok_or_else(|| ToolError::failed("There is no task to plan."))?;
        let plan = steps(input);
        let n = plan.len();
        let t = self.hub.update(id, Some(TaskEvent::PlanUpdated), |t| t.set_plan(plan)).ok_or_else(|| ToolError::failed("The task is no longer running."))?;
        if let Some(step) = t.current_step {
            self.hub.update(id, Some(TaskEvent::StepStarted { step }), |_| {});
        }
        let current = t.current_step.and_then(|i| t.plan.get(i)).map(|s| format!(" Current step: {}.", s.title)).unwrap_or_default();
        Ok(ToolOutput { content: format!("Plan saved ({n} steps).{current}"), summary: format!("{n} steps"), sources: vec![], media: vec![] })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;
    use tokio_util::sync::CancellationToken;

    #[tokio::test]
    async fn plans_are_saved_on_the_running_task() {
        let db = Arc::new(Database::open_in_memory().unwrap());
        let cid = crate::conversations::create(&db.conn().unwrap(), "t", "s", &[]).unwrap().id;
        let hub = Orchestrator::new(db, None);
        let tool = TaskPlanTool::new(hub.clone());
        assert!(super::super::schema::is_strict_compatible(&tool.spec().input_schema));
        let input = json!({"steps": [{"title": "Create file", "status": "completed"}, {"title": "Verify", "status": "active"}]});
        assert!(tool.execute_in(&input, &ToolCtx::default()).await.is_err(), "no task, no plan");
        let t = hub.begin(&cid, "Make notes", &CancellationToken::new()).unwrap();
        let out = tool.execute_in(&input, &ToolCtx { conversation_id: Some(cid), task_id: Some(t.id.clone()) }).await.unwrap();
        assert!(out.content.contains("Current step: Verify"));
        let t = hub.task(&t.id).unwrap();
        assert_eq!((t.plan.len(), t.current_step, t.plan[0].status), (2, Some(1), StepStatus::Completed));
    }
}
