#![forbid(unsafe_code)]

use raccord_planner::{Planner, RenderPlan};
use raccord_rmap::ActionResult;

#[derive(Default)]
pub struct Runtime {
    planner: Planner,
}

impl Runtime {
    pub fn plan(&self, project: &raccord_ir::Project) -> RenderPlan {
        self.planner.plan(project)
    }

    pub fn action_result(&self, result: ActionResult) -> ActionResult {
        result
    }
}
