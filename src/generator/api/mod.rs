//! Everything `api.output` generates (ADR 0016): the infrastructure beside
//! the features ([`shared`]), each table's API feature under
//! `api.output/<module>/`, the root `generated.rs` and `README.md`, and the
//! data layer's `listing.rs` that a feature's list reads.
//!
//! [`Feature`] is the model the renderers share: its endpoints decide the
//! routes, handlers, and service methods.

mod dto;
mod feature;
mod files;
mod handlers;
mod layout;
pub mod listing;
mod root;
mod service;
pub mod shared;

use std::path::PathBuf;

use self::feature::Endpoint;
pub use self::feature::Feature;
use crate::ApiConfig;
use crate::schema::Table;

/// The API features of `tables` (the run's tables, kept ones included), in
/// table order.
pub fn features<'a>(tables: &'a [Table], api: &'a ApiConfig) -> Result<Vec<Feature<'a>>, String> {
    let mut features = Vec::new();
    for table in tables {
        features.extend(Feature::of(table, api)?);
    }
    Ok(features)
}

/// Whether `feature` needs the data layer's `listing.rs`.
pub fn has_listing(feature: &Feature<'_>) -> bool {
    feature.serves(Endpoint::List)
}

/// The files under `api.output`: each feature's, then `generated.rs` and
/// `README.md`.
pub fn render(features: &[Feature<'_>], api: &ApiConfig) -> Vec<(PathBuf, String)> {
    let root = api.output.as_path();
    let mut files = Vec::new();
    for feature in features {
        let dir = root.join(feature.module());
        files.extend([
            (dir.join("mod.rs"), files::render_mod(feature)),
            (dir.join("routes.rs"), handlers::render_routes(feature)),
            (dir.join("handlers.rs"), handlers::render_handlers(feature)),
            (dir.join("dto.rs"), dto::render(feature)),
            (dir.join("model.rs"), files::render_model(feature)),
            (dir.join("repo.rs"), files::render_repo(feature)),
            (dir.join("service_api.rs"), service::render_api(feature)),
            (dir.join("service.rs"), service::render_scaffold(feature)),
        ]);
    }
    files.push((root.join("generated.rs"), root::render_generated(features)));
    files.push((root.join("README.md"), root::render_readme(features)));
    files
}

/// The directories whose generated files a run with `api` owns, besides
/// `output`: `api.output` and the shared tools' directory. A generated file
/// in one that the run doesn't plan is stale.
pub fn owned_dirs(api: &ApiConfig) -> [PathBuf; 2] {
    [api.output.clone(), shared::shared_dir(api)]
}
