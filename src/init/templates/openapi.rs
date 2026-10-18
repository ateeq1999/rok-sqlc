//! The `OpenAPI` document `app.rs` serves at `/api-docs/openapi.json`, with
//! Swagger UI at `/docs/`: the generated features' `GeneratedApiDoc`,
//! merged into [`ApiDoc`], which lists your hand-written paths and schemas.

use utoipa::OpenApi;

use crate::features::GeneratedApiDoc;

/// Your hand-written paths and schemas: list a handler carrying
/// `#[utoipa::path]` in `paths(...)`, and a type deriving `ToSchema` that
/// no path refers to in `components(schemas(...))`. Where yours and the
/// generated document name the same path or schema, yours wins.
#[derive(OpenApi)]
#[openapi(paths(), components(schemas()))]
pub struct ApiDoc;

/// The whole document: [`ApiDoc`] with every generated path and schema
/// merged in, titled and versioned after the crate.
#[must_use]
pub fn document() -> utoipa::openapi::OpenApi {
    let mut doc = ApiDoc::openapi();
    doc.merge(GeneratedApiDoc::openapi());
    doc.info.title = env!("CARGO_PKG_NAME").to_owned();
    doc.info.version = env!("CARGO_PKG_VERSION").to_owned();
    doc
}
