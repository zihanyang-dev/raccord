#![forbid(unsafe_code)]

use raccord_ir::ClipId;
use raccord_timeline::Placement;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Violation {
    Overlap { left: ClipId, right: ClipId },
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ConstraintReport {
    violations: Vec<Violation>,
}

impl ConstraintReport {
    pub fn violations(&self) -> &[Violation] {
        &self.violations
    }

    pub fn is_valid(&self) -> bool {
        self.violations.is_empty()
    }
}

#[derive(Default)]
pub struct Solver;

impl Solver {
    pub fn check(&self, placements: &[Placement]) -> ConstraintReport {
        let mut report = ConstraintReport::default();

        for pair in placements.windows(2) {
            let left = pair[0].range();
            let right = pair[1].range();
            let left_end = left.start().value().saturating_add(left.duration().value());

            if left_end > right.start().value() {
                report.violations.push(Violation::Overlap {
                    left: pair[0].clip().clone(),
                    right: pair[1].clip().clone(),
                });
            }
        }

        report
    }
}
