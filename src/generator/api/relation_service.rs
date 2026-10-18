//! The relationship routes' service methods (see [`super::relations`]):
//! the to-one embedding's joined read, the to-many reads through the data
//! layer's loaders, and the many-to-many writes, which go through the
//! feature's own table's `listing.rs` (also rendered here).

use super::feature::Feature;
use super::layout::{Uses, push_call, push_signature};
use super::relations::{Kind, Relation};
use super::scope;
use super::service::{key_arg, key_param, push_not_owned, push_transaction};
use crate::generator::{keys, param_name, param_type, push_doc, push_fn_head};

/// The relation methods of the service trait.
pub fn render_service(output: &mut String, uses: &mut Uses, feature: &Feature<'_>) {
    for relation in &feature.relations {
        let user = relation.caller(feature).user;
        if user {
            uses.add("crate::shared::auth::AuthUser");
        }
        if let Some(tenant) = relation.tenant {
            scope::import_type(feature, uses, tenant);
        }
        let row = relation.row();
        if relation.target.module_name != feature.module() {
            uses.add(&format!(
                "{}::{}::models::{row}",
                feature.db_path, relation.target.module_name
            ));
        }
        if relation.target_owner.is_some() {
            uses.add(&format!(
                "{}::{}",
                feature.db_path, relation.target.module_name
            ));
        }
        let mut params = vec!["&self".to_owned()];
        params.extend(feature.scope_params(relation.tenant, user));
        params.push(key_param(feature));
        let target = &relation.target.module_name;
        match &relation.kind {
            Kind::ToOne {
                optional,
                row_field,
                tenant,
            } => {
                output.push('\n');
                push_doc(
                    output,
                    4,
                    &[
                        &format!(
                            "`GET {}{}?include={}`: the row with this key and its",
                            feature.url,
                            feature.key_path(),
                            relation.name
                        ),
                        &format!(
                            "`{}`, `None` when the caller can't read it; or `404`.",
                            relation.name
                        ),
                    ],
                );
                let returns = format!("Result<({}, Option<{row}>), AppError>", feature.row());
                let head = format!("async fn get_one_with_{}", relation.name);
                push_signature(output, 4, &head, &params, &returns);
                let mut args = vec!["self.pool()".to_owned()];
                if *tenant {
                    args.push("tenant".to_owned());
                }
                args.push(key_arg(feature));
                let callee = format!("repo::get_with_{}", relation.name);
                push_call(output, 8, "let joined = ", &callee, &args, ".await?;");
                output.push_str("        let joined = joined.ok_or_else(not_found)?;\n");
                if feature.owner.is_some() {
                    output.push_str(&format!(
                        "        if !repo::owned_by(&joined.{row_field}, user.id) {{\n            return Err(not_found());\n        }}\n"
                    ));
                }
                let related = if *optional {
                    format!("joined.{}", relation.name)
                } else {
                    format!("Some(joined.{})", relation.name)
                };
                if relation.target_owner.is_some() {
                    let args = [related, "user.id".to_owned()];
                    let callee = format!("{target}::listing::owned");
                    push_call(output, 8, "let related = ", &callee, &args, ";");
                } else {
                    output.push_str(&format!("        let related = {related};\n"));
                }
                output.push_str(&format!(
                    "        Ok((joined.{row_field}, related))\n    }}\n"
                ));
            }
            Kind::ToMany { key, tenant, .. } => {
                output.push('\n');
                push_doc(
                    output,
                    4,
                    &[&format!(
                        "`GET {}{}/{}`: the row's `{}`, or `404`.",
                        feature.url,
                        feature.key_path(),
                        relation.segment(),
                        relation.name
                    )],
                );
                let returns = format!("Result<Vec<{row}>, AppError>");
                let head = format!("async fn list_{}", relation.name);
                push_signature(output, 4, &head, &params, &returns);
                push_read_row(output, feature);
                output.push_str("        let rows = std::slice::from_ref(&row);\n");
                let mut args = vec!["self.pool()".to_owned()];
                if *tenant {
                    args.push("tenant".to_owned());
                }
                args.push("rows".to_owned());
                let callee = format!("repo::load_{}", relation.name);
                push_call(output, 8, "let mut loaded = ", &callee, &args, ".await?;");
                let binding = if relation.target_owner.is_some() {
                    "let mut related"
                } else {
                    "let related"
                };
                let remove = format!("&row.{}", key.rust_name);
                push_call(output, 8, "let related = ", "loaded.remove", &[remove], ";");
                output.push_str(&format!(
                    "        {binding} = related.unwrap_or_default();\n"
                ));
                if relation.target_owner.is_some() {
                    let callee = format!("{target}::listing::retain_owned");
                    let args = ["&mut related".to_owned(), "user.id".to_owned()];
                    push_call(output, 8, "", &callee, &args, ";");
                }
                output.push_str("        Ok(related)\n    }\n");
                render_writes(output, uses, feature, relation, &params);
            }
        }
    }
}

/// Reads the feature's row by `key` into `row`: `404` when there is none
/// (or another caller owns it).
fn push_read_row(output: &mut String, feature: &Feature<'_>) {
    let tenant = scope::tenant_arg(feature.table);
    let mut args = vec!["self.pool()".to_owned()];
    args.extend(tenant.strip_suffix(", ").map(str::to_owned));
    args.push(key_arg(feature));
    let callee = if feature.owner.is_some() {
        args.push("user.id".to_owned());
        "repo::get_owned"
    } else {
        "repo::get_by_id"
    };
    push_call(output, 8, "let row = ", callee, &args, ".await?;");
    output.push_str("        let row = row.ok_or_else(not_found)?;\n");
}

/// The many-to-many writes' `_in` methods and their transactions.
fn render_writes(
    output: &mut String,
    uses: &mut Uses,
    feature: &Feature<'_>,
    relation: &Relation<'_>,
    params: &[String],
) {
    let Some(writes) = relation.writes() else {
        return;
    };
    uses.add("sqlx::PgConnection");
    scope::import_type(feature, uses, writes.other);
    let other = relation.target_key();
    let join_tenant = scope::tenant_arg(writes.join);
    let target_tenant = scope::tenant_arg(relation.target);
    let segment = relation.segment();
    let path = format!("{}{}/{segment}", feature.url, feature.key_path());
    // The table a missing, unseen or unpaired `other` belongs to, as a
    // string literal.
    let related = format!("{:?}", relation.target.display_name());
    let mut methods = Vec::new();
    if relation.syncs() {
        methods.push((
            "sync",
            format!("`PUT {path}`: pairs the row with exactly `ids`, or `404`."),
            format!("ids: Vec<{other}>"),
        ));
    }
    if relation.attaches() {
        methods.push((
            "attach",
            format!("`PUT {path}/{{other}}`: pairs the row with `other`, or `404`."),
            format!("other: {other}"),
        ));
    }
    methods.push((
        "detach",
        format!("`DELETE {path}/{{other}}`: unpairs `other` from the row, or `404`."),
        format!("other: {other}"),
    ));
    for (name, doc, input) in methods {
        let method = format!("{name}_{}", relation.name);
        let mut inputs: Vec<String> = params[1..].to_vec();
        inputs.push(input);
        output.push('\n');
        push_doc(
            output,
            4,
            &[
                &format!(
                    "What [`{method}`](Self::{method}) does, on `conn`: the caller's connection or"
                ),
                "transaction. Override this one to change it.",
            ],
        );
        let mut in_params = vec!["&self".to_owned(), "conn: &mut PgConnection".to_owned()];
        in_params.extend(inputs.iter().cloned());
        let returns = "Result<(), AppError>";
        push_signature(
            output,
            4,
            &format!("async fn {method}_in"),
            &in_params,
            returns,
        );
        // The row must be there, and the caller's.
        let mut args = vec!["&mut *conn".to_owned()];
        args.extend(
            scope::tenant_arg(feature.table)
                .strip_suffix(", ")
                .map(str::to_owned),
        );
        args.push(key_arg(feature));
        if feature.owner.is_some() {
            args.push("user.id".to_owned());
            push_call(
                output,
                8,
                "let found = ",
                "repo::lock_owned",
                &args,
                ".await?;",
            );
        } else {
            push_call(
                output,
                8,
                "let found = ",
                "repo::exists_by_id",
                &args,
                ".await?;",
            );
        }
        push_not_owned(output, "found");
        if name != "sync" && relation.target_owner.is_some() {
            let mut args = vec!["&mut *conn".to_owned()];
            args.extend(target_tenant.strip_suffix(", ").map(str::to_owned));
            args.extend(["other".to_owned(), "user.id".to_owned()]);
            let callee = format!("{}::listing::lock_owned", relation.target.module_name);
            push_call(output, 8, "let theirs = ", &callee, &args, ".await?;");
            output.push_str(&format!(
                "        if !theirs {{\n            return Err(not_found_in({related}));\n        }}\n"
            ));
        }
        let mut args = vec!["&mut *conn".to_owned()];
        args.extend(join_tenant.strip_suffix(", ").map(str::to_owned));
        args.push(key_arg(feature));
        let callee = format!("repo::{method}");
        match name {
            "sync" => {
                args.push("&ids".to_owned());
                push_call(output, 8, "", &callee, &args, ".await?;");
                output.push_str("        Ok(())\n    }\n");
            }
            "attach" => {
                args.push("other".to_owned());
                push_call(output, 8, "", &callee, &args, ".await?;");
                output.push_str("        Ok(())\n    }\n");
            }
            _ => {
                args.push("other".to_owned());
                push_call(output, 8, "let detached = ", &callee, &args, ".await?;");
                output.push_str(&format!(
                    "        if detached {{\n            Ok(())\n        }} else {{\n            Err(not_found_in({related}))\n        }}\n    }}\n"
                ));
            }
        }
        output.push('\n');
        push_doc(
            output,
            4,
            &[
                &doc,
                &format!("Runs [`{method}_in`](Self::{method}_in) in a transaction of its own."),
            ],
        );
        push_transaction(output, &method, &inputs, "()");
    }
}

/// The `listing.rs` writes of a many-to-many relation: its join table's
/// `attach`, `detach` and `sync`, taking the feature's key first, so the
/// feature calls them through its own table's module.
pub fn render_pair_writes(output: &mut String, relation: &Relation<'_>) {
    let Some(writes) = relation.writes() else {
        return;
    };
    let join = writes.join;
    let call = |name: &str| format!("super::super::{}::write::{name}", join.module_name);
    let tenant = keys::tenant_params(join);
    let mut leading = vec!["executor".to_owned()];
    leading.extend(join.tenant().map(param_name));
    let own = param_name(writes.own);
    let other = param_name(writes.other);
    let mut params = tenant.clone();
    params.push(format!("{own}: {}", param_type(writes.own)));
    params.push(format!("{other}: {}", param_type(writes.other)));
    let pair = if writes.near {
        [own.clone(), other.clone()]
    } else {
        [other.clone(), own.clone()]
    };
    let target = crate::generator::doc_name(&relation.target.display_name());
    let join_name = crate::generator::doc_name(&join.display_name());
    let mut functions = Vec::new();
    if relation.attaches() {
        functions.push((
            "attach",
            vec![
                format!("Pairs the row with the {target} row through {join_name}, and returns"),
                "whether the pair is new.".to_owned(),
            ],
        ));
    }
    functions.push((
        "detach",
        vec![format!(
            "Unpairs the row from the {target} row, and returns whether they were paired."
        )],
    ));
    for (name, doc) in functions {
        output.push('\n');
        let doc: Vec<&str> = doc.iter().map(String::as_str).collect();
        push_doc(output, 0, &doc);
        let function = format!("{name}_{}", relation.name);
        push_fn_head(output, &function, &params, "Result<bool, sqlx::Error>");
        let mut args = leading.clone();
        args.extend(pair.iter().cloned());
        push_call(output, 4, "", &call(name), &args, ".await");
        output.push_str("}\n");
    }
    if relation.syncs() {
        output.push('\n');
        push_doc(
            output,
            0,
            &[
                &format!("Pairs the row with exactly the {target} rows `ids` names, through"),
                &format!("{join_name}, in one statement."),
            ],
        );
        let mut params = tenant;
        params.push(format!("{own}: {}", param_type(writes.own)));
        params.push(format!("ids: &[{}]", writes.other.value_type()));
        let function = format!("sync_{}", relation.name);
        push_fn_head(output, &function, &params, "Result<(), sqlx::Error>");
        let mut args = leading;
        args.extend([own, "ids".to_owned()]);
        push_call(output, 4, "", &call("sync"), &args, ".await");
        output.push_str("}\n");
    }
}
