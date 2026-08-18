#![forbid(unsafe_code)]

use raccord_constraints::Solver;
use raccord_ir::Project;
use raccord_timeline::Resolver;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RenderUnit {
    pub start_frame: u64,
    pub frame_count: u64,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RenderPlan {
    pub units: Vec<RenderUnit>,
}

#[derive(Default)]
pub struct Planner {
    resolver: Resolver,
    constraints: Solver,
}

impl Planner {
    pub fn plan(&self, project: &Project) -> RenderPlan {
        let placements = self.resolver.resolve(project);
        let _report = self.constraints.check(&placements);

        RenderPlan {
            units: placements
                .iter()
                .map(|placement| RenderUnit {
                    start_frame: placement.range().start().value(),
                    frame_count: placement.range().duration().value(),
                })
                .collect(),
        }
    }
}
