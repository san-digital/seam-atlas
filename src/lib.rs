#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

pub mod cliffs;
pub mod diff;
pub mod dsl;
pub mod json;
pub mod model;
pub mod partition;

pub use cliffs::{find_cliffs, find_cliffs_in_partition, CliffEdge};
pub use diff::{diff_pipelines, DiffClass, DiffReport};
pub use dsl::{parse_policy, ParseError};
pub use json::{cliffs_to_json, diff_to_json};
pub use model::{CmpOp, Domain, FieldDef, Outcome, Pipeline, Pred, Rule, Schema, Value};
pub use partition::{compile_partition, CoordSpan, LeafRegion, Partition};
