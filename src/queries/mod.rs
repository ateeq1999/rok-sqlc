//! SQLC-style annotated query files: parsing annotations, then analyzing
//! each query against the schema so code generation can emit typed
//! functions.

pub mod analyze;
pub mod annotations;

pub use analyze::{AnalyzedQuery, OutputColumn, Param, ResultShape, analyze_queries};
pub use annotations::{QueryDef, QueryFailure, ReturnMode, parse_query_file};
