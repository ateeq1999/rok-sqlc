//! The OpenAPI side of a feature's handlers (ADR 0016): the
//! `#[utoipa::path]` above each one, documenting its method, absolute path,
//! tag, operation id (the handler's name), params, request body, `bearer_auth`
//! when it needs a caller, and exactly the statuses it can answer, each
//! error with the wire standard's `ErrorBody`. [`super::operation`] renders
//! them.

use super::feature::{Endpoint, Feature, Lookup};
use super::layout::Uses;
use super::operation::{Header, Operation, Security, access_errors, column_description};
use super::relations::{Action, Relation};
use crate::ApiAccess;
use crate::schema::RelationshipKind;

/// The `Location` header of a created row.
const LOCATION: Header = ("Location", "The created row's URL.");

/// Whether writing `feature`'s row can break a unique key or a foreign key.
fn insert_conflicts(feature: &Feature<'_>) -> bool {
    let table = feature.table;
    !table.primary_key.is_empty() || !table.unique_keys.is_empty() || update_conflicts(feature)
}

/// Whether changing `feature`'s row can break a unique or foreign key.
fn update_conflicts(feature: &Feature<'_>) -> bool {
    !feature.table.unique_keys.is_empty() || !feature.table.foreign_keys.is_empty()
}

/// Whether restoring `feature`'s soft-deleted row can break a unique key,
/// which a live row took meanwhile.
fn restore_conflicts(feature: &Feature<'_>) -> bool {
    !feature.table.unique_keys.is_empty()
}

/// Whether deleting `feature`'s row can break another table's foreign key.
fn delete_conflicts(feature: &Feature<'_>) -> bool {
    feature
        .table
        .relationships
        .iter()
        .any(|relationship| relationship.kind != RelationshipKind::BelongsTo)
}

/// The operation of `endpoint`.
fn endpoint_operation(feature: &Feature<'_>, endpoint: Endpoint) -> Operation {
    let path = endpoint.path(&feature.key_path());
    let op = Operation::new(
        feature,
        endpoint.method(),
        &path,
        endpoint.name().to_owned(),
        feature.caller(),
    );
    let response = feature.response();
    let page = format!("PaginatedResponse<{response}>");
    match endpoint {
        Endpoint::List => {
            op.query(feature.list_query())
                .answers(200, "One page of rows.", Some(page))
        }
        Endpoint::Search => op.json(feature.search_body()).answers(
            200,
            "One page of the rows the filter matches.",
            Some(page),
        ),
        Endpoint::Create => {
            let mut op = op
                .json(feature.create_dto())
                .answers(201, "The row, created.", Some(response))
                .conflicts(insert_conflicts(feature));
            if feature.key.is_some() {
                op.headers.push(LOCATION);
            }
            op.etag(feature)
        }
        Endpoint::GetOne => {
            let op = op
                .keyed(feature)
                .errors(&[404])
                .answers(200, "The row.", Some(response))
                .etag(feature);
            with_includes(op, feature)
        }
        Endpoint::Update => {
            let mut op = op.keyed(feature);
            if feature.version.is_some() {
                op.params.push(
                    "(\"If-Match\" = String, Header, description = \"The row's `ETag`.\")"
                        .to_owned(),
                );
                op.errors.extend([412, 428]);
            }
            op.json(feature.update_dto())
                .errors(&[404])
                .answers(200, "The row, changed.", Some(response))
                .etag(feature)
                .conflicts(update_conflicts(feature))
        }
        Endpoint::Remove => {
            let op = op.keyed(feature).errors(&[404]);
            let op = if feature.soft_delete {
                op.query(feature.remove_query()).answers(
                    204,
                    "The row, soft-deleted (deleted with `?permanent=true`).",
                    None,
                )
            } else {
                op.answers(204, "The row, deleted.", None)
            };
            op.conflicts(delete_conflicts(feature))
        }
        Endpoint::Restore => op
            .keyed(feature)
            .errors(&[404])
            .answers(200, "The row, restored.", Some(response))
            .etag(feature)
            .conflicts(restore_conflicts(feature)),
    }
}

/// `GET /{key}` with its `?include=`, when the feature has to-one
/// relations: each may need a stricter caller than the row.
fn with_includes(mut op: Operation, feature: &Feature<'_>) -> Operation {
    let includes = feature.includes();
    if includes.is_empty() {
        return op;
    }
    op = op.query(feature.get_query());
    op.description = "The row, with the relationship `?include=` names embedded.";
    for relation in includes {
        let caller = relation.caller(feature);
        op.errors.extend(access_errors(caller.access));
        if op.security == Security::None && caller.access != ApiAccess::Public {
            op.security = Security::Optional;
        }
    }
    op
}

/// The operation of a `GET /by-<column>/{value}` route.
fn lookup_operation(feature: &Feature<'_>, lookup: &Lookup<'_>) -> Operation {
    Operation::new(
        feature,
        "get",
        &lookup.path(),
        lookup.function.clone(),
        feature.caller(),
    )
    .errors(&[404])
    .answers(200, "The row.", Some(feature.response()))
    .etag(feature)
    .path_param(
        "value",
        lookup.column,
        &column_description(feature.table, lookup.column),
    )
}

/// The operation of a to-many relation's `action`.
fn relation_operation(feature: &Feature<'_>, relation: &Relation<'_>, action: Action) -> Operation {
    let op = Operation::new(
        feature,
        action.method(),
        &relation.path(feature, action),
        relation.handler(action),
        relation.caller(feature),
    )
    .keyed(feature)
    .errors(&[404]);
    let op = match relation.writes().filter(|_| action.names_other()) {
        Some(writes) => {
            // Writes need the related table's single key, so it has one.
            let key = relation.target.key_columns();
            let description = key
                .first()
                .map_or_else(String::new, |key| column_description(relation.target, key));
            op.path_param("other", writes.other, &description)
        }
        None => op,
    };
    match action {
        Action::List => op.answers(
            200,
            "The related rows.",
            Some(format!("Vec<{}>", relation.response())),
        ),
        Action::Sync => op
            .json(format!("IdList<{}>", relation.target_key()))
            .errors(&[409])
            .answers(204, "The row, paired with exactly the rows named.", None),
        Action::Attach => op.errors(&[409]).answers(204, "The rows, paired.", None),
        Action::Detach => op.answers(204, "The rows, unpaired.", None),
    }
}

/// Adds the `#[utoipa::path]` of `endpoint`'s handler, with
/// `feature.openapi`.
pub fn push_endpoint(
    output: &mut String,
    uses: &mut Uses,
    feature: &Feature<'_>,
    endpoint: Endpoint,
) {
    if feature.openapi {
        endpoint_operation(feature, endpoint).push(output, uses);
    }
}

/// Adds the `#[utoipa::path]` of a lookup's handler, with
/// `feature.openapi`.
pub fn push_lookup(
    output: &mut String,
    uses: &mut Uses,
    feature: &Feature<'_>,
    lookup: &Lookup<'_>,
) {
    if feature.openapi {
        lookup_operation(feature, lookup).push(output, uses);
    }
}

/// Adds the `#[utoipa::path]` of the relation's `action` handler, with
/// `feature.openapi`.
pub fn push_relation(
    output: &mut String,
    uses: &mut Uses,
    feature: &Feature<'_>,
    relation: &Relation<'_>,
    action: Action,
) {
    if feature.openapi {
        relation_operation(feature, relation, action).push(output, uses);
    }
}

/// The handlers whose paths `GeneratedApiDoc` lists, in the order
/// `handlers.rs` holds them.
pub fn handler_names(feature: &Feature<'_>) -> Vec<String> {
    let mut names: Vec<String> = feature
        .endpoints
        .iter()
        .map(|endpoint| endpoint.name().to_owned())
        .collect();
    names.extend(feature.lookups.iter().map(|lookup| lookup.function.clone()));
    for relation in &feature.relations {
        names.extend(
            relation
                .actions()
                .into_iter()
                .map(|action| relation.handler(action)),
        );
    }
    names
}
