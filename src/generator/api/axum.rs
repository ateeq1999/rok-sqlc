//! What `[api] framework = "axum"` renders whole, besides the handlers
//! (which [`super::framework`] adapts): each feature's `routes.rs`, a
//! `Router<AppState>`, and `generated.rs`, with [`Services`](self) and the
//! `FromRef` impls that hand each handler its service (0.7, decision A3).
//!
//! The routers and the `FromRef` impls name the scaffolded, user-owned
//! `crate::app::AppState`, which must hold the services as `services`.

use super::feature::Feature;
use super::handlers::route_list;
use super::layout::push_struct_lit;
use crate::ApiConfig;
use crate::generator::{CHAIN_WIDTH, FN_CALL_WIDTH, HEADER, MAX_WIDTH, push_doc, use_list};

/// Renders `routes.rs`: every route under its full URL, the methods of one
/// path chained on one `MethodRouter`. axum answers a malformed path or
/// query through `shared/validation.rs`'s `Path` and `Query`.
pub fn render_routes(feature: &Feature<'_>) -> String {
    // Each path once, in `route_list`'s order, with its methods.
    let mut paths: Vec<(String, Vec<(&str, String)>)> = Vec::new();
    for (path, method, handler) in route_list(feature) {
        let url = format!("{}{path}", feature.url);
        match paths.iter_mut().find(|(other, _)| *other == url) {
            Some((_, methods)) => methods.push((method, handler)),
            None => paths.push((url, vec![(method, handler)])),
        }
    }
    // Each chain starts with a function of `axum::routing`; the methods
    // after it are `MethodRouter`'s.
    let mut methods: Vec<String> = paths
        .iter()
        .filter_map(|(_, methods)| methods.first().map(|(method, _)| (*method).to_owned()))
        .collect();
    methods.sort();
    methods.dedup();
    let mut output = format!(
        "{HEADER}\n//! The `{}` routes, under `{}`.\n\nuse axum::Router;\n",
        feature.table.display_name(),
        feature.url
    );
    if !methods.is_empty() {
        output.push_str(&use_list("axum::routing", &methods));
    }
    output.push('\n');
    if !paths.is_empty() {
        output.push_str("use super::handlers;\n");
    }
    output.push_str("use crate::app::AppState;\n\n");
    push_doc(
        &mut output,
        0,
        &[&format!(
            "Every `{}` endpoint, under `{}`.",
            feature.table.display_name(),
            feature.url
        )],
    );
    output.push_str("pub fn routes() -> Router<AppState> {\n");
    let routes: Vec<(String, String)> = paths
        .iter()
        .map(|(path, methods)| {
            let chain: Vec<String> = methods
                .iter()
                .map(|(method, handler)| format!("{method}(handlers::{handler})"))
                .collect();
            (crate::generator::sql::str_literal(path), chain.join("."))
        })
        .collect();
    let one_line: String = routes
        .iter()
        .map(|(path, chain)| format!(".route({path}, {chain})"))
        .collect();
    let chain = format!("Router::new(){one_line}");
    let simple = routes
        .iter()
        .all(|(path, chain)| path.len() + 2 + chain.len() <= FN_CALL_WIDTH);
    if routes.len() <= 1 && simple && chain.len() <= CHAIN_WIDTH && 4 + chain.len() <= MAX_WIDTH {
        output.push_str(&format!("    {chain}\n}}\n"));
        return output;
    }
    output.push_str("    Router::new()");
    for (path, chain) in &routes {
        let line = format!("        .route({path}, {chain})");
        if path.len() + 2 + chain.len() <= FN_CALL_WIDTH && line.len() <= MAX_WIDTH {
            output.push('\n');
            output.push_str(&line);
            continue;
        }
        output.push_str(&format!("\n        .route(\n            {path},\n"));
        if chain.len() <= CHAIN_WIDTH && 13 + chain.len() <= MAX_WIDTH {
            output.push_str(&format!("            {chain},\n"));
        } else {
            // One method per line below the first.
            let mut parts = chain.split(").");
            let first = parts.next().unwrap_or_default();
            output.push_str(&format!("            {first})"));
            for part in parts {
                output.push_str(&format!("\n                .{part}"));
                if !part.ends_with(')') {
                    output.push(')');
                }
            }
            output.push_str(",\n");
        }
        output.push_str("        )");
    }
    output.push_str("\n}\n");
    output
}

/// Renders `generated.rs`: the features, `Services`, `register_services`,
/// the `FromRef` impls, `routes` and, under `[api] openapi`,
/// `GeneratedApiDoc`.
pub fn render_generated(features: &[Feature<'_>], api: &ApiConfig) -> String {
    let mut output = format!(
        "{HEADER}
//! Every generated API feature, declared from its own folder; [`Services`],
//! which `AppState` holds as `services`; and the two functions `app.rs`
//! calls: [`register_services`] and [`routes`].

"
    );
    if !features.is_empty() {
        output.push_str("use std::sync::Arc;\n\n");
    }
    output.push_str("use axum::Router;\n");
    if !features.is_empty() {
        output.push_str("use axum::extract::FromRef;\n");
    }
    output.push_str("use sqlx::PgPool;\n");
    if api.openapi {
        output.push_str(super::api_doc::USES);
    }
    output.push_str("\nuse crate::app::AppState;\n");
    if !features.is_empty() {
        output.push('\n');
    }
    // The services, imported so the impls' heads stay short.
    let mut services: Vec<(&str, String)> = features
        .iter()
        .map(|feature| (feature.module(), feature.service()))
        .collect();
    services.sort();
    for (module, service) in &services {
        output.push_str(&format!("use self::{module}::{service};\n"));
    }
    super::root::push_modules(&mut output, features);

    output.push_str(
        "\n/// Every feature's service, built once by [`register_services`]. Each\n/// handler takes its own through `State<Arc<...>>`.\n#[derive(Clone)]\n",
    );
    if features.is_empty() {
        output.push_str("pub struct Services {}\n");
    } else {
        output.push_str("pub struct Services {\n");
        for feature in features {
            push_doc(
                &mut output,
                4,
                &[&format!("The `{}` service.", feature.module())],
            );
            output.push_str(&format!(
                "    pub {}: Arc<{}>,\n",
                feature.module(),
                feature.service()
            ));
        }
        output.push_str("}\n");
    }

    let pool = if features.is_empty() { "_pool" } else { "pool" };
    output.push_str(&format!(
        "\n/// Builds every feature's service, each over `pool`.\n#[must_use]\npub fn register_services({pool}: &PgPool) -> Services {{\n"
    ));
    for feature in features {
        let module = feature.module();
        let service = feature.service();
        let call = format!("{service}::new(pool.clone())");
        let line = format!("    let {module} = Arc::new({call});");
        if call.len() <= FN_CALL_WIDTH && line.len() <= MAX_WIDTH {
            output.push_str(&format!("{line}\n"));
        } else {
            // A long name: built first, so each statement wraps simply.
            if format!("    let service = {call};").len() <= MAX_WIDTH {
                output.push_str(&format!("    let service = {call};\n"));
            } else if format!("        {call};").len() <= MAX_WIDTH {
                output.push_str(&format!("    let service =\n        {call};\n"));
            } else {
                output.push_str(&format!(
                    "    let service =\n        {service}::new(\n            pool.clone(),\n        );\n"
                ));
            }
            output.push_str(&format!("    let {module} = Arc::new(service);\n"));
        }
    }
    let fields: Vec<String> = features
        .iter()
        .map(|feature| feature.module().to_owned())
        .collect();
    push_struct_lit(&mut output, 4, "Services", &fields);
    output.push_str("}\n");

    for feature in features {
        let module = feature.module();
        let service = feature.service();
        output.push_str(&format!(
            "\nimpl FromRef<Services> for Arc<{service}> {{\n    fn from_ref(services: &Services) -> Self {{\n        Arc::clone(&services.{module})\n    }}\n}}\n"
        ));
        output.push_str(&format!(
            "\nimpl FromRef<AppState> for Arc<{service}> {{\n    fn from_ref(state: &AppState) -> Self {{\n        Self::from_ref(&state.services)\n    }}\n}}\n"
        ));
    }

    output
        .push_str("\n/// Every feature's routes, merged.\npub fn routes() -> Router<AppState> {\n");
    let merges: String = features
        .iter()
        .map(|feature| format!(".merge({}::routes())", feature.module()))
        .collect();
    let chain = format!("Router::new(){merges}");
    if features.len() <= 1 && chain.len() <= CHAIN_WIDTH {
        output.push_str(&format!("    {chain}\n"));
    } else {
        output.push_str("    Router::new()\n");
        for feature in features {
            output.push_str(&format!("        .merge({}::routes())\n", feature.module()));
        }
    }
    output.push_str("}\n");
    if api.openapi {
        output.push_str(&super::api_doc::render(features));
    }
    output
}
