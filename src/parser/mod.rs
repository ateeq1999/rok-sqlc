pub mod diagnostics;
pub mod naming;
pub mod postgres;

pub use diagnostics::TableFailure;
pub use postgres::{
    ParsedSchema, SchemaSource, parse_schema, parse_sources, parse_sources_with_modules,
};
