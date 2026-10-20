//! What the web framework changes in a feature's handlers (0.7, decision
//! A2): how a handler takes its service, its path and its query, what it
//! returns, and how it answers. The handler renderers ([`super::handlers`],
//! [`super::relation_routes`]) ask [`ApiFramework`] for each piece, so the
//! rest of a handler (its scope, its service call, its body) is rendered
//! once for both frameworks. The routes and `generated.rs` differ whole:
//! [`super::axum`] renders axum's.

use super::layout::Uses;
use crate::ApiFramework;

/// A request part a handler takes: its parameter, and the expression of its
/// value in the handler's body.
pub struct Extracted {
    param: String,
    pub value: String,
    /// What the parameter's type needs imported.
    import: Option<&'static str>,
}

impl Extracted {
    /// The handler parameter, importing its type.
    pub fn param(&self, uses: &mut Uses) -> String {
        if let Some(import) = self.import {
            uses.add(import);
        }
        self.param.clone()
    }

    /// `let {binding} = {value};` when the value needs taking out first
    /// (actix-web's `path.into_inner()`); nothing when the parameter
    /// already binds it (axum's `Path(key)`).
    pub fn push_let(&self, output: &mut String, binding: &str) {
        if self.value != binding {
            output.push_str(&format!("    let {binding} = {};\n", self.value));
        }
    }
}

impl ApiFramework {
    /// The imports every handler file needs.
    pub(super) fn handler_uses(self, uses: &mut Uses) {
        match self {
            ApiFramework::Actix => {
                uses.add("actix_web::HttpResponse");
                uses.add("actix_web::web");
            }
            ApiFramework::Axum => {
                uses.add("axum::extract::State");
                uses.add("axum::response::IntoResponse");
                uses.add("axum::response::Response");
                uses.add("std::sync::Arc");
            }
        }
    }

    /// The handler parameter taking the feature's `service`.
    pub(super) fn service_param(self, service: &str) -> String {
        match self {
            ApiFramework::Actix => format!("service: web::Data<{service}>"),
            ApiFramework::Axum => format!("State(service): State<Arc<{service}>>"),
        }
    }

    /// The path, of type `ty`, as the value `binding` (a pattern, such as
    /// `(key, other)`, for a tuple).
    pub(super) fn path(self, binding: &str, ty: &str) -> Extracted {
        match self {
            ApiFramework::Actix => Extracted {
                param: format!("path: web::Path<{ty}>"),
                value: "path.into_inner()".to_owned(),
                import: None,
            },
            ApiFramework::Axum => Extracted {
                param: format!("Path({binding}): Path<{ty}>"),
                value: binding.to_owned(),
                import: Some("crate::shared::validation::Path"),
            },
        }
    }

    /// The query string, of type `ty`.
    pub(super) fn query(self, ty: &str) -> Extracted {
        match self {
            ApiFramework::Actix => Extracted {
                param: format!("query: web::Query<{ty}>"),
                value: "query.into_inner()".to_owned(),
                import: None,
            },
            ApiFramework::Axum => Extracted {
                param: format!("Query(query): Query<{ty}>"),
                value: "query".to_owned(),
                import: Some("crate::shared::validation::Query"),
            },
        }
    }

    /// A handler's return type.
    pub(super) fn returns(self) -> &'static str {
        match self {
            ApiFramework::Actix => "Result<HttpResponse, AppError>",
            ApiFramework::Axum => "Result<Response, AppError>",
        }
    }

    /// Answers `body` as JSON with `status` (`Ok` or `Created`) and
    /// `headers`, each `(header::NAME, value)`.
    pub(super) fn push_answer(
        self,
        output: &mut String,
        uses: &mut Uses,
        status: &str,
        headers: &[&str],
        body: &str,
    ) {
        match self {
            ApiFramework::Actix => push_actix_answer(output, uses, status, headers, body),
            ApiFramework::Axum => push_axum_answer(output, uses, status, headers, body),
        }
    }

    /// Answers `204 No Content`.
    pub(super) fn push_no_content(self, output: &mut String, uses: &mut Uses) {
        match self {
            ApiFramework::Actix => output.push_str("    Ok(HttpResponse::NoContent().finish())\n"),
            ApiFramework::Axum => {
                uses.add("axum::http::StatusCode");
                output.push_str("    Ok(StatusCode::NO_CONTENT.into_response())\n");
            }
        }
    }
}

/// `Ok(HttpResponse::<status>()...json(<body>))`, one header per line.
fn push_actix_answer(
    output: &mut String,
    uses: &mut Uses,
    status: &str,
    headers: &[&str],
    body: &str,
) {
    if headers.is_empty() {
        output.push_str(&format!("    Ok(HttpResponse::{status}().json({body}))\n"));
        return;
    }
    uses.add("actix_web::http::header");
    output.push_str(&format!("    Ok(HttpResponse::{status}()\n"));
    for header in headers {
        output.push_str(&format!("        .insert_header({header})\n"));
    }
    output.push_str(&format!("        .json({body}))\n"));
}

/// `Ok(Json(<body>).into_response())`, or the status and the headers
/// first, as a tuple bound to `answer`.
fn push_axum_answer(
    output: &mut String,
    uses: &mut Uses,
    status: &str,
    headers: &[&str],
    body: &str,
) {
    uses.add("axum::Json");
    let mut parts = Vec::new();
    if status == "Created" {
        uses.add("axum::http::StatusCode");
        parts.push("StatusCode::CREATED".to_owned());
    }
    if !headers.is_empty() {
        uses.add("axum::http::header");
        output.push_str(&format!("    let headers = [{}];\n", headers.join(", ")));
        parts.push("headers".to_owned());
    }
    if parts.is_empty() {
        output.push_str(&format!("    Ok(Json({body}).into_response())\n"));
        return;
    }
    parts.push(format!("Json({body})"));
    output.push_str(&format!("    let answer = ({});\n", parts.join(", ")));
    output.push_str("    Ok(answer.into_response())\n");
}
