//! Access, ownership and hidden columns: `/api/v1/accounts` is admin only
//! and hides `password_hash`; `/api/v1/tasks` takes any signed-in caller
//! and scopes every row to its owner, `owner_id`.

mod common;

use actix_web::http::{Method, StatusCode};
use common::{Database, assert_error, call, call_as, token};
use rok_sqlc_api_app::shared::auth::Role;
use serde_json::{Value, json};
use uuid::Uuid;

const ACCOUNTS: &str = "/api/v1/accounts";
const TASKS: &str = "/api/v1/tasks";

/// An admin's token.
fn admin() -> String {
    token(Uuid::now_v7(), Role::Admin)
}

/// The field names of a row answer, sorted.
fn fields(row: &Value) -> Vec<String> {
    let mut names: Vec<String> = row
        .as_object()
        .expect("an object")
        .keys()
        .cloned()
        .collect();
    names.sort();
    names
}

/// Creates an account as an admin and returns its id.
async fn account(db: &Database, email: &str) -> Uuid {
    let answer = call_as(
        db,
        Some(&admin()),
        Method::POST,
        ACCOUNTS,
        Some(json!({"email": email, "password_hash": "secret hash"})),
    )
    .await;
    assert_eq!(answer.status, StatusCode::CREATED, "{answer:?}");
    let id = answer.body["id"].as_str().expect("an id");
    Uuid::parse_str(id).expect("a UUID")
}

#[actix_web::test]
async fn admin_endpoints_answer_401_without_a_token_and_403_to_a_user() {
    let Some(db) = Database::fresh().await else {
        return;
    };
    let id = account(&db, "a@example.com").await;
    let one = format!("{ACCOUNTS}/{id}");
    let user = token(Uuid::now_v7(), Role::User);
    let body = || Some(json!({"email": "b@example.com", "password_hash": "x"}));
    let requests = [
        (Method::GET, ACCOUNTS.to_owned(), None),
        (Method::POST, ACCOUNTS.to_owned(), body()),
        (Method::GET, one.clone(), None),
        (
            Method::PATCH,
            one.clone(),
            Some(json!({"email": "c@example.com"})),
        ),
        (Method::DELETE, one.clone(), None),
    ];
    for (method, uri, body) in requests {
        let anonymous = call(&db, method.clone(), &uri, body.clone()).await;
        assert_error(&anonymous, StatusCode::UNAUTHORIZED, "unauthorized");
        let bad = call_as(&db, Some("not a token"), method.clone(), &uri, body.clone()).await;
        assert_error(&bad, StatusCode::UNAUTHORIZED, "unauthorized");
        let forbidden = call_as(&db, Some(&user), method, &uri, body).await;
        assert_error(&forbidden, StatusCode::FORBIDDEN, "forbidden");
    }
    // Nothing changed: the account is still there, as it was.
    let fetched = call_as(&db, Some(&admin()), Method::GET, &one, None).await;
    assert_eq!(fetched.status, StatusCode::OK, "{fetched:?}");
    assert_eq!(fetched.body["email"], "a@example.com");
    db.drop().await;
}

#[actix_web::test]
async fn hidden_columns_are_written_but_never_answered() {
    let Some(db) = Database::fresh().await else {
        return;
    };
    let admin = admin();
    let created = call_as(
        &db,
        Some(&admin),
        Method::POST,
        ACCOUNTS,
        Some(json!({"email": "a@example.com", "password_hash": "first hash"})),
    )
    .await;
    assert_eq!(created.status, StatusCode::CREATED, "{created:?}");
    assert_eq!(fields(&created.body), ["email", "id"]);
    let id = created.body["id"].as_str().expect("an id").to_owned();
    let one = format!("{ACCOUNTS}/{id}");

    // The create body needs it, like the column.
    let missing = call_as(
        &db,
        Some(&admin),
        Method::POST,
        ACCOUNTS,
        Some(json!({"email": "b@example.com"})),
    )
    .await;
    assert_eq!(
        missing.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{missing:?}"
    );

    let fetched = call_as(&db, Some(&admin), Method::GET, &one, None).await;
    assert_eq!(fetched.status, StatusCode::OK, "{fetched:?}");
    assert_eq!(fields(&fetched.body), ["email", "id"]);

    let updated = call_as(
        &db,
        Some(&admin),
        Method::PATCH,
        &one,
        Some(json!({"password_hash": "second hash"})),
    )
    .await;
    assert_eq!(updated.status, StatusCode::OK, "{updated:?}");
    assert_eq!(fields(&updated.body), ["email", "id"]);
    let stored: String = sqlx::query_scalar("SELECT password_hash FROM accounts")
        .fetch_one(&db.pool)
        .await
        .expect("the stored hash");
    assert_eq!(stored, "second hash");

    let list = call_as(&db, Some(&admin), Method::GET, ACCOUNTS, None).await;
    assert_eq!(list.status, StatusCode::OK, "{list:?}");
    let items = list.body["items"].as_array().expect("items");
    assert_eq!(items.len(), 1);
    assert_eq!(fields(&items[0]), ["email", "id"]);

    // `sort` can't name a hidden column.
    let sorted = call_as(
        &db,
        Some(&admin),
        Method::GET,
        &format!("{ACCOUNTS}?sort=password_hash"),
        None,
    )
    .await;
    assert_error(&sorted, StatusCode::BAD_REQUEST, "bad_request");
    db.drop().await;
}

#[actix_web::test]
async fn owned_endpoints_answer_401_without_a_token() {
    let Some(db) = Database::fresh().await else {
        return;
    };
    let one = format!("{TASKS}/{}", Uuid::now_v7());
    let requests = [
        (Method::GET, TASKS.to_owned(), None),
        (Method::POST, TASKS.to_owned(), Some(json!({"title": "t"}))),
        (Method::GET, one.clone(), None),
        (Method::PATCH, one.clone(), Some(json!({"title": "u"}))),
        (Method::DELETE, one, None),
    ];
    for (method, uri, body) in requests {
        let answer = call(&db, method, &uri, body).await;
        assert_error(&answer, StatusCode::UNAUTHORIZED, "unauthorized");
    }
    db.drop().await;
}

#[actix_web::test]
async fn create_sets_the_owner_from_the_caller() {
    let Some(db) = Database::fresh().await else {
        return;
    };
    let alice = account(&db, "alice@example.com").await;
    let bob = account(&db, "bob@example.com").await;
    let alice_token = token(alice, Role::User);
    // An `owner_id` in the body is no field of the create body: the owner
    // is the caller.
    let created = call_as(
        &db,
        Some(&alice_token),
        Method::POST,
        TASKS,
        Some(json!({"title": "Write tests", "owner_id": bob})),
    )
    .await;
    assert_eq!(created.status, StatusCode::CREATED, "{created:?}");
    assert_eq!(created.body["owner_id"], alice.to_string());
    assert_eq!(created.body["done"], false);
    let id = created.body["id"].as_str().expect("an id");
    assert_eq!(created.location, Some(format!("{TASKS}/{id}")));

    // An update can't hand the row to someone else.
    let one = format!("{TASKS}/{id}");
    let updated = call_as(
        &db,
        Some(&alice_token),
        Method::PATCH,
        &one,
        Some(json!({"done": true, "owner_id": bob})),
    )
    .await;
    assert_eq!(updated.status, StatusCode::OK, "{updated:?}");
    assert_eq!(updated.body["done"], true);
    assert_eq!(updated.body["owner_id"], alice.to_string());
    db.drop().await;
}

#[actix_web::test]
async fn another_callers_rows_are_not_found() {
    let Some(db) = Database::fresh().await else {
        return;
    };
    let alice = token(account(&db, "alice@example.com").await, Role::User);
    let bob = token(account(&db, "bob@example.com").await, Role::User);
    let create = |token: String, title: &'static str| {
        let db = &db;
        async move {
            let answer = call_as(
                db,
                Some(&token),
                Method::POST,
                TASKS,
                Some(json!({"title": title})),
            )
            .await;
            assert_eq!(answer.status, StatusCode::CREATED, "{answer:?}");
            answer.body["id"].as_str().expect("an id").to_owned()
        }
    };
    let alices = create(alice.clone(), "Alice's").await;
    create(bob.clone(), "Bob's").await;
    let one = format!("{TASKS}/{alices}");

    // Bob sees only his own rows, and none of Alice's.
    let list = call_as(&db, Some(&bob), Method::GET, TASKS, None).await;
    assert_eq!(list.status, StatusCode::OK, "{list:?}");
    assert_eq!(list.body["total"], 1);
    assert_eq!(list.body["items"][0]["title"], "Bob's");
    let fetched = call_as(&db, Some(&bob), Method::GET, &one, None).await;
    assert_error(&fetched, StatusCode::NOT_FOUND, "not_found");
    let patch = Some(json!({"title": "Bob was here"}));
    let updated = call_as(&db, Some(&bob), Method::PATCH, &one, patch).await;
    assert_error(&updated, StatusCode::NOT_FOUND, "not_found");
    let removed = call_as(&db, Some(&bob), Method::DELETE, &one, None).await;
    assert_error(&removed, StatusCode::NOT_FOUND, "not_found");
    // An admin is a caller like any other: not Alice.
    let admin = call_as(&db, Some(&admin()), Method::GET, &one, None).await;
    assert_error(&admin, StatusCode::NOT_FOUND, "not_found");

    // Alice's row is untouched, and hers to read, list and delete.
    let fetched = call_as(&db, Some(&alice), Method::GET, &one, None).await;
    assert_eq!(fetched.status, StatusCode::OK, "{fetched:?}");
    assert_eq!(fetched.body["title"], "Alice's");
    let list = call_as(&db, Some(&alice), Method::GET, TASKS, None).await;
    assert_eq!(list.body["total"], 1);
    assert_eq!(list.body["items"][0]["title"], "Alice's");
    let removed = call_as(&db, Some(&alice), Method::DELETE, &one, None).await;
    assert_eq!(removed.status, StatusCode::NO_CONTENT, "{removed:?}");
    let gone = call_as(&db, Some(&alice), Method::GET, &one, None).await;
    assert_error(&gone, StatusCode::NOT_FOUND, "not_found");
    db.drop().await;
}
