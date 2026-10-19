//! The relationship routes of a feature (see [`super::relations`]): their
//! routes, handlers, and request and response bodies. Their service
//! methods are [`super::relation_service`]'s.

use heck::ToSnakeCase;

use super::feature::Feature;
use super::handlers::{own_scope, push_answer};
use super::layout::{Uses, push_call, push_signature};
use super::paths;
use super::relations::{Action, Kind, Relation};
use super::scope::{self, Extraction};
use crate::generator::openapi::{push_into_params, push_to_schema};
use crate::generator::{MAX_WIDTH, push_doc};

/// The relation routes: `(path, method, handler)`.
pub fn routes(feature: &Feature<'_>) -> Vec<(String, &'static str, String)> {
    feature
        .relations
        .iter()
        .flat_map(|relation| {
            relation.actions().into_iter().map(move |action| {
                (
                    relation.path(feature, action),
                    action.method(),
                    relation.handler(action),
                )
            })
        })
        .collect()
}

/// Adds four spaces before each line of `text`.
fn indented(text: &str) -> String {
    text.lines()
        .map(|line| {
            if line.is_empty() {
                "\n".to_owned()
            } else {
                format!("    {line}\n")
            }
        })
        .collect()
}

/// The arguments a relation's service method takes before the key, given
/// the handler's own scope: the tenant and the caller, each extracted from
/// `req` when the handler's own scope lacks it (`extract` gets the
/// statements doing that, the caller's first).
fn relation_args(
    feature: &Feature<'_>,
    uses: &mut Uses,
    relation: &Relation<'_>,
    extract: &mut String,
) -> Vec<String> {
    let mut args = Vec::new();
    if let Some(tenant) = relation.tenant {
        args.push("tenant.into_inner()".to_owned());
        if feature.tenant.is_none() {
            uses.add("crate::shared::tenant::Tenant");
            uses.add("actix_web::FromRequest");
            scope::import_type(feature, uses, tenant);
        }
    }
    let caller = relation.caller(feature);
    if let Some(Extraction {
        extractor,
        binding,
        arg,
    }) = caller.extraction()
    {
        // The handler's own extractor already hands over this caller.
        if caller != feature.caller() {
            uses.add("actix_web::FromRequest");
            uses.add(&format!("crate::shared::auth::{extractor}"));
            if arg.is_some() {
                extract.push_str(&format!(
                    "    let {binding} = {extractor}::extract(&req).await?;\n"
                ));
            } else {
                extract.push_str(&format!("    {extractor}::extract(&req).await?;\n"));
            }
        }
        args.extend(arg.map(str::to_owned));
    }
    if let Some(tenant) = relation.tenant.filter(|_| feature.tenant.is_none()) {
        extract.push_str(&format!(
            "    let tenant = Tenant::<{}>::extract(&req).await?;\n",
            tenant.value_type()
        ));
    }
    args
}

/// `GET /{key}` with `?include=`: the row alone, or with one to-one
/// relationship embedded, after the related table's access.
pub fn render_get_one(output: &mut String, uses: &mut Uses, feature: &Feature<'_>) {
    let key = feature.key_type().unwrap_or_default();
    uses.add(&format!("super::model::{key}"));
    uses.add(&format!("super::dto::{}", feature.get_query()));
    uses.add(&format!("super::dto::{}", feature.include_enum()));
    let own = own_scope(feature, uses);
    let mut arms = Vec::new();
    let mut extracts = false;
    for relation in feature.includes() {
        let mut arm = String::new();
        let mut extract = String::new();
        let mut args = relation_args(feature, uses, relation, &mut extract);
        extracts |= !extract.is_empty();
        arm.push_str(&extract);
        args.push("key".to_owned());
        let callee = format!("service.get_one_with_{}", relation.name);
        push_call(
            &mut arm,
            4,
            "let (row, related) = ",
            &callee,
            &args,
            ".await?;",
        );
        let mut headers = Vec::new();
        if let Some(version) = feature.version {
            uses.add("crate::shared::response::etag");
            arm.push_str(&format!(
                "    let etag = etag(&row.{});\n",
                version.rust_name
            ));
            headers.push("(header::ETAG, etag)");
        }
        let response = relation.response();
        if relation.target.module_name != feature.module() {
            uses.add(&format!("super::super::{response}"));
        }
        arm.push_str(&format!(
            "    let response = {} {{\n        row: {}::from(row),\n        {}: related.map({response}::from),\n    }};\n",
            relation.with_response(feature),
            feature.response(),
            relation.name
        ));
        uses.add(&format!("super::dto::{}", relation.with_response(feature)));
        push_answer(&mut arm, uses, "Ok", &headers, "response");
        arms.push((relation.variant(), arm));
    }
    let mut params = Vec::new();
    if extracts {
        uses.add("actix_web::HttpRequest");
        params.push("req: HttpRequest".to_owned());
    }
    params.push(format!("service: web::Data<{}>", feature.service()));
    params.extend(own.params);
    params.push(format!("path: web::Path<{key}>"));
    params.push(format!("query: web::Query<{}>", feature.get_query()));
    push_signature(
        output,
        0,
        "pub async fn get_one",
        &params,
        "Result<HttpResponse, AppError>",
    );
    output.push_str("    let key = path.into_inner();\n");
    output.push_str("    let Some(include) = query.into_inner().include else {\n");
    let mut args = own.args;
    args.push("key".to_owned());
    let mut plain = String::new();
    push_call(
        &mut plain,
        4,
        "let row = ",
        "service.get_one",
        &args,
        ".await?;",
    );
    let mut headers = Vec::new();
    if let Some(version) = feature.version {
        uses.add("crate::shared::response::etag");
        plain.push_str(&format!(
            "    let etag = etag(&row.{});\n",
            version.rust_name
        ));
        headers.push("(header::ETAG, etag)");
    }
    plain.push_str(&format!(
        "    let response = {}::from(row);\n",
        feature.response()
    ));
    push_answer(&mut plain, uses, "Ok", &headers, "response");
    let plain = plain.replacen("    Ok(HttpResponse", "    return Ok(HttpResponse", 1);
    let plain = match plain.rfind('\n') {
        Some(end) if plain[..end].ends_with(')') => format!("{};\n", &plain[..end]),
        _ => plain,
    };
    output.push_str(&indented(&plain));
    output.push_str("    };\n");
    output.push_str("    match include {\n");
    for (variant, arm) in arms {
        output.push_str(&format!(
            "        {}::{variant} => {{\n",
            feature.include_enum()
        ));
        output.push_str(&indented(&indented(&arm)));
        output.push_str("        }\n");
    }
    output.push_str("    }\n}\n");
}

/// The handlers of the to-many relations.
pub fn render_handlers(output: &mut String, uses: &mut Uses, feature: &Feature<'_>) {
    let key = feature.key_type().unwrap_or_default();
    for relation in &feature.relations {
        let Kind::ToMany { .. } = relation.kind else {
            continue;
        };
        uses.add(&format!("super::model::{key}"));
        let scope = scope::handler(feature, uses, relation.tenant, relation.caller(feature));
        let response = relation.response();
        if relation.target.module_name != feature.module() {
            uses.add(&format!("super::super::{response}"));
        }
        let path = format!("{}{}", feature.url, relation.path(feature, Action::List));
        let base = |action: Action| {
            let mut params = vec![format!("service: web::Data<{}>", feature.service())];
            params.extend(scope.params.iter().cloned());
            (format!("pub async fn {}", relation.handler(action)), params)
        };

        output.push('\n');
        push_doc(
            output,
            0,
            &[&format!(
                "`GET {path}`: the row's `{}`, `200` with a JSON array.",
                relation.name
            )],
        );
        paths::push_relation(output, uses, feature, relation, Action::List);
        let (head, mut params) = base(Action::List);
        params.push(format!("path: web::Path<{key}>"));
        push_signature(output, 0, &head, &params, "Result<HttpResponse, AppError>");
        let mut args = scope.args.clone();
        args.push("path.into_inner()".to_owned());
        let callee = format!("service.list_{}", relation.name);
        push_call(output, 4, "let rows = ", &callee, &args, ".await?;");
        // A bare JSON array; the right-hand side moves to its own line when
        // the type makes the line too long, as rustfmt does.
        let head = format!("    let response: Vec<{response}> =");
        let rows = "rows.into_iter().map(Into::into).collect();";
        if head.len() + 1 + rows.len() <= MAX_WIDTH {
            output.push_str(&format!("{head} {rows}\n"));
        } else {
            output.push_str(&format!("{head}\n        {rows}\n"));
        }
        output.push_str("    Ok(HttpResponse::Ok().json(response))\n}\n");

        let Some(writes) = relation.writes() else {
            continue;
        };
        let other = relation.target_key();
        if let Some(id) = writes.other.typed_id.as_ref() {
            if id.module == feature.module() {
                uses.add(&format!("super::model::{}", id.name));
            } else {
                uses.add(&format!(
                    "{}::{}::models::{}",
                    feature.db_path, id.module, id.name
                ));
            }
        }
        if relation.syncs() {
            output.push('\n');
            push_doc(
                output,
                0,
                &[
                    &format!(
                        "`PUT {path}`: pairs the row with exactly the `{}`",
                        relation.name
                    ),
                    "rows `{\"ids\": [...]}` names, `204`.",
                ],
            );
            paths::push_relation(output, uses, feature, relation, Action::Sync);
            let (head, mut params) = base(Action::Sync);
            uses.add("crate::shared::validation::IdList");
            uses.add("crate::shared::validation::ValidatedJson");
            params.push(format!("path: web::Path<{key}>"));
            params.push(format!("body: ValidatedJson<IdList<{other}>>"));
            push_signature(output, 0, &head, &params, "Result<HttpResponse, AppError>");
            output.push_str("    let ids = body.into_inner().ids;\n");
            let mut args = scope.args.clone();
            args.extend(["path.into_inner()".to_owned(), "ids".to_owned()]);
            let callee = format!("service.sync_{}", relation.name);
            push_call(output, 4, "", &callee, &args, ".await?;");
            output.push_str("    Ok(HttpResponse::NoContent().finish())\n}\n");
        }
        let pairings = [
            (Action::Attach, "pairs the row with the"),
            (Action::Detach, "unpairs the row from the"),
        ];
        let actions = relation.actions();
        for (action, what) in pairings {
            if !actions.contains(&action) {
                continue;
            }
            let method = action.method().to_uppercase();
            output.push('\n');
            push_doc(
                output,
                0,
                &[&format!(
                    "`{method} {path}/{{other}}`: {what} `{}` row, `204`.",
                    relation.target.display_name()
                )],
            );
            paths::push_relation(output, uses, feature, relation, action);
            let (head, mut params) = base(action);
            params.push(pair_path(&key, &other));
            push_signature(output, 0, &head, &params, "Result<HttpResponse, AppError>");
            output.push_str("    let (key, other) = path.into_inner();\n");
            let mut args = scope.args.clone();
            args.extend(["key".to_owned(), "other".to_owned()]);
            let callee = format!("service.{}", relation.handler(action));
            push_call(output, 4, "", &callee, &args, ".await?;");
            output.push_str("    Ok(HttpResponse::NoContent().finish())\n}\n");
        }
    }
}

/// The `dto.rs` items of the to-one relations: `?include=`'s query string
/// and names, and each embedding's response.
pub fn render_dto(output: &mut String, uses: &mut Uses, feature: &Feature<'_>) {
    let includes = feature.includes();
    if includes.is_empty() {
        return;
    }
    let include = feature.include_enum();
    output.push('\n');
    push_doc(
        output,
        0,
        &[&format!(
            "`GET {}{}`'s query string: `?include=` embeds one to-one relationship.",
            feature.url,
            feature.key_path()
        )],
    );
    output.push_str("#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Deserialize)]\n");
    push_into_params(output, feature.openapi);
    output.push_str(&format!(
        "pub struct {} {{\n    /// The relationship to embed.\n    pub include: Option<{include}>,\n}}\n",
        feature.get_query()
    ));
    output.push('\n');
    push_doc(
        output,
        0,
        &["The to-one relationships `?include=` takes, in `snake_case`."],
    );
    output.push_str("#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]\n");
    push_to_schema(output, feature.openapi);
    output.push_str("#[serde(rename_all = \"snake_case\")]\n");
    output.push_str(&format!("pub enum {include} {{\n"));
    for relation in &includes {
        push_doc(
            output,
            4,
            &[&format!("The `{}` relationship.", relation.name)],
        );
        output.push_str(&format!("    {},\n", relation.variant()));
    }
    output.push_str("}\n");
    for relation in &includes {
        let response = relation.response();
        if relation.target.module_name != feature.module() {
            uses.add(&format!("super::super::{response}"));
        }
        output.push('\n');
        push_doc(
            output,
            0,
            &[&format!(
                "A `{}` row with its `{}`, as `?include={}` answers it.",
                feature.table.display_name(),
                relation.name,
                relation.name.to_snake_case()
            )],
        );
        output.push_str("#[derive(Debug, Clone, PartialEq, serde::Serialize)]\n");
        push_to_schema(output, feature.openapi);
        output.push_str(&format!(
            "pub struct {} {{\n    /// The row.\n    #[serde(flatten)]\n    pub row: {},\n",
            relation.with_response(feature),
            feature.response()
        ));
        push_doc(
            output,
            4,
            &[&format!(
                "Its `{}`, or `null` when there is none the caller may read.",
                relation.name
            )],
        );
        output.push_str(&format!(
            "    pub {}: Option<{response}>,\n}}\n",
            relation.name
        ));
    }
}

/// The `path` parameter of an attach or detach: the row's key and the
/// related row's, the tuple broken over lines as rustfmt breaks it when the
/// parameter is too long.
fn pair_path(key: &str, other: &str) -> String {
    let pair = format!("{key}, {other}");
    if pair.len() <= crate::generator::FN_CALL_WIDTH {
        format!("path: web::Path<({pair})>")
    } else {
        format!("path: web::Path<(\n        {key},\n        {other},\n    )>")
    }
}
