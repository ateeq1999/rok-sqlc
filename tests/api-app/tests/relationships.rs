//! Relationship routes: a to-one relationship embedded by `?include=`, a
//! has-many one under `/{key}/<name>`, and many-to-many ones read, synced,
//! attached and detached. Related rows answer through their own feature's
//! response, and their table's access and ownership apply.

mod common;

use actix_web::http::{Method, StatusCode};
use common::{Database, assert_error, call, call_as, create, get, token};
use rok_sqlc_api_app::shared::auth::Role;
use serde_json::{Value, json};
use uuid::Uuid;

const NOTES: &str = "/api/v1/notes";
const LABELS: &str = "/api/v1/labels";
const TASKS: &str = "/api/v1/tasks";

/// A note's id.
async fn note(db: &Database, slug: &str) -> String {
    let row = create(db, NOTES, json!({"slug": slug, "title": slug})).await;
    row["id"].as_str().expect("an id").to_owned()
}

/// A label's id.
async fn label(db: &Database, name: &str) -> i64 {
    let row = create(db, LABELS, json!({"name": name})).await;
    row["id"].as_i64().expect("an id")
}

/// An account with this id, made in the database, and a token for it.
async fn account(db: &Database, role: Role) -> (Uuid, String) {
    let id = Uuid::now_v7();
    sqlx::query("INSERT INTO accounts (id, email, password_hash) VALUES ($1, $2, 'hash')")
        .bind(id)
        .bind(format!("{id}@example.com"))
        .execute(&db.pool)
        .await
        .expect("an account");
    (id, token(id, role))
}

/// A task of the caller whose `token` it is; returns its id.
async fn task(db: &Database, token: &str, title: &str) -> String {
    let body = Some(json!({"title": title}));
    let answer = call_as(db, Some(token), Method::POST, TASKS, body).await;
    assert_eq!(answer.status, StatusCode::CREATED, "{answer:?}");
    answer.body["id"].as_str().expect("an id").to_owned()
}

/// The values of `field` in a `{"items": [...]}` answer, in order.
fn items(body: &Value, field: &str) -> Vec<Value> {
    body["items"]
        .as_array()
        .expect("items")
        .iter()
        .map(|item| item[field].clone())
        .collect()
}

#[actix_web::test]
async fn include_embeds_a_to_one_relationship() {
    let Some(db) = Database::fresh().await else {
        return;
    };
    let id = note(&db, "first").await;
    let member = create(
        &db,
        "/api/v1/members",
        json!({"team": "core", "note_id": id}),
    )
    .await;
    let one = format!("/api/v1/members/core/{id}");
    let plain = get(&db, &one).await;
    assert_eq!(plain.body, member);
    let included = get(&db, &format!("{one}?include=note")).await;
    assert_eq!(included.status, StatusCode::OK, "{included:?}");
    assert_eq!(included.body["team"], "core");
    assert_eq!(included.body["note"]["slug"], "first");
    let unknown = get(&db, &format!("{one}?include=nothing")).await;
    assert_error(&unknown, StatusCode::BAD_REQUEST, "bad_request");
    let missing = get(&db, &format!("/api/v1/members/other/{id}?include=note")).await;
    assert_error(&missing, StatusCode::NOT_FOUND, "not_found");
    db.drop().await;
}

#[actix_web::test]
async fn include_applies_the_related_tables_access_and_hides_its_columns() {
    let Some(db) = Database::fresh().await else {
        return;
    };
    let (_, user) = account(&db, Role::User).await;
    let (admin_id, admin) = account(&db, Role::Admin).await;
    let theirs = task(&db, &user, "theirs").await;
    let mine = task(&db, &admin, "mine").await;
    // `accounts` is admin only: a user can't read their owner row.
    let uri = format!("{TASKS}/{theirs}?include=owner");
    let forbidden = call_as(&db, Some(&user), Method::GET, &uri, None).await;
    assert_error(&forbidden, StatusCode::FORBIDDEN, "forbidden");
    let plain = call_as(&db, Some(&user), Method::GET, &format!("{TASKS}/{theirs}"), None).await;
    assert_eq!(plain.status, StatusCode::OK, "{plain:?}");
    // An admin reads their own task's owner, without `password_hash`, and
    // still can't reach another caller's task.
    let uri = format!("{TASKS}/{mine}?include=owner");
    let included = call_as(&db, Some(&admin), Method::GET, &uri, None).await;
    assert_eq!(included.status, StatusCode::OK, "{included:?}");
    assert_eq!(included.body["owner"]["id"], admin_id.to_string());
    assert!(included.body["owner"].get("password_hash").is_none());
    let uri = format!("{TASKS}/{theirs}?include=owner");
    let other = call_as(&db, Some(&admin), Method::GET, &uri, None).await;
    assert_error(&other, StatusCode::NOT_FOUND, "not_found");
    db.drop().await;
}

#[actix_web::test]
async fn has_many_lists_the_related_rows_under_a_renamed_route() {
    let Some(db) = Database::fresh().await else {
        return;
    };
    let id = note(&db, "first").await;
    let other = note(&db, "second").await;
    for (team, note_id) in [("core", &id), ("docs", &id), ("core", &other)] {
        create(
            &db,
            "/api/v1/members",
            json!({"team": team, "note_id": note_id}),
        )
        .await;
    }
    let answer = get(&db, &format!("{NOTES}/{id}/memberships")).await;
    assert_eq!(answer.status, StatusCode::OK, "{answer:?}");
    assert_eq!(items(&answer.body, "team"), [json!("core"), json!("docs")]);
    let missing = get(&db, &format!("{NOTES}/{}/memberships", Uuid::now_v7())).await;
    assert_error(&missing, StatusCode::NOT_FOUND, "not_found");
    // The inferred name is gone.
    let old = get(&db, &format!("{NOTES}/{id}/members")).await;
    assert_eq!(old.status, StatusCode::NOT_FOUND, "{old:?}");
    db.drop().await;
}

#[actix_web::test]
async fn has_many_shows_only_the_callers_own_rows() {
    let Some(db) = Database::fresh().await else {
        return;
    };
    let (user_id, user) = account(&db, Role::User).await;
    let (admin_id, admin) = account(&db, Role::Admin).await;
    task(&db, &user, "theirs").await;
    task(&db, &admin, "mine").await;
    // `accounts` is admin only, and `tasks` rows are their owner's: an admin
    // listing another account's tasks sees none of them.
    let theirs = format!("/api/v1/accounts/{user_id}/tasks");
    let answer = call_as(&db, Some(&admin), Method::GET, &theirs, None).await;
    assert_eq!(answer.status, StatusCode::OK, "{answer:?}");
    assert_eq!(answer.body, json!({"items": []}));
    let mine = format!("/api/v1/accounts/{admin_id}/tasks");
    let answer = call_as(&db, Some(&admin), Method::GET, &mine, None).await;
    assert_eq!(items(&answer.body, "title"), [json!("mine")]);
    let forbidden = call_as(&db, Some(&user), Method::GET, &theirs, None).await;
    assert_error(&forbidden, StatusCode::FORBIDDEN, "forbidden");
    db.drop().await;
}

#[actix_web::test]
async fn many_to_many_reads_syncs_attaches_and_detaches() {
    let Some(db) = Database::fresh().await else {
        return;
    };
    let id = note(&db, "first").await;
    let red = label(&db, "red").await;
    let blue = label(&db, "blue").await;
    let green = label(&db, "green").await;
    let labels = format!("{NOTES}/{id}/labels");
    let names = |body: &Value| items(body, "name");

    let synced = call(&db, Method::PUT, &labels, Some(json!({"ids": [red, blue]}))).await;
    assert_eq!(synced.status, StatusCode::NO_CONTENT, "{synced:?}");
    assert_eq!(names(&get(&db, &labels).await.body), [json!("red"), json!("blue")]);
    let synced = call(&db, Method::PUT, &labels, Some(json!({"ids": [blue]}))).await;
    assert_eq!(synced.status, StatusCode::NO_CONTENT, "{synced:?}");
    assert_eq!(names(&get(&db, &labels).await.body), [json!("blue")]);

    let attached = call(&db, Method::PUT, &format!("{labels}/{green}"), None).await;
    assert_eq!(attached.status, StatusCode::NO_CONTENT, "{attached:?}");
    let again = call(&db, Method::PUT, &format!("{labels}/{green}"), None).await;
    assert_eq!(again.status, StatusCode::NO_CONTENT, "{again:?}");
    assert_eq!(
        names(&get(&db, &labels).await.body),
        [json!("blue"), json!("green")]
    );
    // The other side reads the pairing too.
    let notes = get(&db, &format!("{LABELS}/{green}/notes")).await;
    assert_eq!(items(&notes.body, "slug"), [json!("first")]);

    let detached = call(&db, Method::DELETE, &format!("{labels}/{blue}"), None).await;
    assert_eq!(detached.status, StatusCode::NO_CONTENT, "{detached:?}");
    let unpaired = call(&db, Method::DELETE, &format!("{labels}/{blue}"), None).await;
    assert_error(&unpaired, StatusCode::NOT_FOUND, "not_found");
    assert_eq!(names(&get(&db, &labels).await.body), [json!("green")]);

    let missing = format!("{NOTES}/{}/labels", Uuid::now_v7());
    assert_error(&get(&db, &missing).await, StatusCode::NOT_FOUND, "not_found");
    let no_label = call(&db, Method::PUT, &format!("{labels}/999999"), None).await;
    assert_error(&no_label, StatusCode::CONFLICT, "conflict");
    let bad = call(&db, Method::PUT, &labels, Some(json!({"ids": "red"}))).await;
    assert_error(&bad, StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
    db.drop().await;
}

#[actix_web::test]
async fn many_to_many_keeps_other_owners_rows_out_of_reach() {
    let Some(db) = Database::fresh().await else {
        return;
    };
    let (_, alice) = account(&db, Role::User).await;
    let (_, bob) = account(&db, Role::User).await;
    let red = label(&db, "red").await;
    let hers = task(&db, &alice, "hers").await;
    let his = task(&db, &bob, "his").await;
    let tasks = format!("{LABELS}/{red}/tasks");

    // `tasks` needs a caller, even from public `labels`.
    assert_error(&get(&db, &tasks).await, StatusCode::UNAUTHORIZED, "unauthorized");
    for (token, id) in [(&alice, &hers), (&bob, &his)] {
        let uri = format!("{tasks}/{id}");
        let answer = call_as(&db, Some(token), Method::PUT, &uri, None).await;
        assert_eq!(answer.status, StatusCode::NO_CONTENT, "{answer:?}");
    }
    let seen = call_as(&db, Some(&alice), Method::GET, &tasks, None).await;
    assert_eq!(items(&seen.body, "title"), [json!("hers")]);
    // Bob's task is a 404 to Alice, attached or not.
    let uri = format!("{tasks}/{his}");
    for method in [Method::PUT, Method::DELETE] {
        let answer = call_as(&db, Some(&alice), method, &uri, None).await;
        assert_error(&answer, StatusCode::NOT_FOUND, "not_found");
    }
    // From the task's side: only its owner reads or syncs its labels.
    let labels = format!("{TASKS}/{his}/labels");
    let answer = call_as(&db, Some(&bob), Method::GET, &labels, None).await;
    assert_eq!(items(&answer.body, "name"), [json!("red")]);
    let answer = call_as(&db, Some(&alice), Method::GET, &labels, None).await;
    assert_error(&answer, StatusCode::NOT_FOUND, "not_found");
    let body = Some(json!({"ids": []}));
    let answer = call_as(&db, Some(&alice), Method::PUT, &labels, body.clone()).await;
    assert_error(&answer, StatusCode::NOT_FOUND, "not_found");
    let answer = call_as(&db, Some(&bob), Method::PUT, &labels, body).await;
    assert_eq!(answer.status, StatusCode::NO_CONTENT, "{answer:?}");
    let answer = call_as(&db, Some(&bob), Method::GET, &labels, None).await;
    assert_eq!(answer.body, json!({"items": []}));
    db.drop().await;
}
