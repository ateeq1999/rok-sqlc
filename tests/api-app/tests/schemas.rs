//! The OpenAPI schemas the generated types derive (milestone 2) describe
//! what the API sends and takes: typed ids as their inner type, nullable
//! columns as `[type, "null"]`, enums with their exact labels, the foreign
//! types in their JSON form, and the query strings in the query.

use rok_sqlc_api_app::db::types::{GadgetMood, GadgetSize};
use rok_sqlc_api_app::features::documents::dto::{DocumentResponse, GetDocumentQuery};
use rok_sqlc_api_app::features::gadgets::dto::{GadgetResponse, UpdateGadgetDto};
use rok_sqlc_api_app::features::labels::dto::LabelResponse;
use rok_sqlc_api_app::features::notes::dto::{ListNotesQuery, NoteResponse, SearchNotesBody};
use rok_sqlc_api_app::shared::response::{ErrorPayload, PaginatedResponse};
use serde_json::{Value, json};
use utoipa::{IntoParams, PartialSchema};

/// `T`'s schema as JSON.
fn schema<T: PartialSchema>() -> Value {
    serde_json::to_value(T::schema()).unwrap_or_default()
}

/// The schema of `T`'s property `name`, without its description (the
/// field's doc comment).
fn property<T: PartialSchema>(name: &str) -> Value {
    let mut property = schema::<T>()["properties"][name].clone();
    if let Some(object) = property.as_object_mut() {
        object.remove("description");
    }
    property
}

#[test]
fn typed_ids_are_their_inner_type() {
    let uuid = json!({"type": "string", "format": "uuid"});
    assert_eq!(property::<NoteResponse>("id"), uuid);
    assert_eq!(
        property::<LabelResponse>("id"),
        json!({"type": "integer", "format": "int64"})
    );
    // A nullable foreign key.
    assert_eq!(
        property::<DocumentResponse>("task_id"),
        json!({"type": ["string", "null"], "format": "uuid"})
    );
}

#[test]
fn columns_are_described_by_their_json_form() {
    assert_eq!(
        property::<NoteResponse>("created_at"),
        json!({"type": "string", "format": "date-time"})
    );
    assert_eq!(
        property::<NoteResponse>("body"),
        json!({"type": ["string", "null"]})
    );
    assert_eq!(
        property::<GadgetResponse>("made_on"),
        json!({"type": ["string", "null"], "format": "date"})
    );
    // `rust_decimal` serializes decimals as text, and an address is text.
    assert_eq!(
        property::<GadgetResponse>("price"),
        json!({"type": ["string", "null"]})
    );
    assert_eq!(
        property::<GadgetResponse>("host"),
        json!({"type": ["string", "null"]})
    );
    // JSON is any value.
    assert_eq!(property::<GadgetResponse>("specs"), json!({}));
    let required = &schema::<NoteResponse>()["required"];
    assert_eq!(
        required,
        &json!(["id", "slug", "title", "priority", "created_at"])
    );
}

#[test]
fn enums_keep_their_exact_labels() {
    assert_eq!(
        schema::<GadgetMood>(),
        json!({"type": "string", "enum": ["happy", "so-so"], "description": "The `gadget_mood` enum type."})
    );
    assert_eq!(schema::<GadgetSize>()["enum"], json!(["small", "Large"]));
    assert_eq!(
        property::<GadgetResponse>("mood"),
        json!({"$ref": "#/components/schemas/GadgetMood"})
    );
}

#[test]
fn an_update_body_requires_nothing_and_takes_null() {
    let update = schema::<UpdateGadgetDto>();
    assert_eq!(update.get("required"), None, "{update}");
    assert_eq!(update["properties"]["host"]["type"], json!(["string", "null"]));
}

#[test]
fn the_error_body_holds_free_form_details() {
    let payload = schema::<ErrorPayload>();
    assert_eq!(payload["required"], json!(["code", "message"]));
    assert_eq!(
        payload["properties"]["details"]["type"],
        json!(["object", "null"])
    );
}

#[test]
fn a_page_holds_its_items() {
    let page = schema::<PaginatedResponse<NoteResponse>>();
    assert_eq!(
        page["required"],
        json!(["items", "page", "per_page", "total"])
    );
    assert_eq!(
        page["properties"]["items"]["items"],
        json!({"$ref": "#/components/schemas/NoteResponse"})
    );
}

#[test]
fn the_search_body_defaults_to_no_conditions() {
    let body = schema::<SearchNotesBody>();
    assert_eq!(body["properties"]["filter"]["default"], json!({}));
    assert_eq!(body.get("required"), None, "{body}");
}

#[test]
fn query_strings_are_parameters_in_the_query() {
    let params = serde_json::to_value(ListNotesQuery::into_params(|| None)).unwrap_or_default();
    let names: Vec<&str> = params
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|param| param["name"].as_str())
        .collect();
    assert_eq!(names, ["page", "per_page", "q", "sort", "order"]);
    for param in params.as_array().into_iter().flatten() {
        assert_eq!(param["in"], "query", "{param}");
        assert_eq!(param["required"], false, "{param}");
    }
    assert_eq!(
        params[3]["schema"],
        json!({"$ref": "#/components/schemas/NoteSort"})
    );
    assert_eq!(
        params[4]["schema"],
        json!({"$ref": "#/components/schemas/SortOrder"})
    );

    let params = serde_json::to_value(GetDocumentQuery::into_params(|| None)).unwrap_or_default();
    assert_eq!(params[0]["name"], "include");
    assert_eq!(params[0]["in"], "query");
    assert_eq!(
        params[0]["schema"],
        json!({"$ref": "#/components/schemas/DocumentInclude"})
    );
}
