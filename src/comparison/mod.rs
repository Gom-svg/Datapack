mod engine;
mod model;
mod temp;

pub(crate) use engine::{compare_path, CompareOptions, ComparisonMode};
// The full DTO graph is staged here for the application facade introduced in
// this phase; not every nested type is consumed by the current CLI adapter.
#[allow(unused_imports)]
pub use model::{
    ComparisonLimitationV1, ComparisonMethodologyV1, ComparisonReportV1, ComparisonScopeV1,
    ComparisonWinnersV1, CompetitorReportV1, CompetitorValidationV1, TimingReportV1,
};
