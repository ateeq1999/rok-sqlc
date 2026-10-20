//! The generated shared tools under axum: the list query and its page, the
//! paginated response, the `Tenant` extractor, and the `Path` and `Query`
//! extractors answering what doesn't parse as actix-web does. No database
//! needed. (`tests/api-app/tests/shared.rs` is actix-web's.)

use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::Body;
use axum::extract::Request;
use axum::http::StatusCode;
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::routing::get;
use rok_sqlc_api_app_axum::db::search::{Direction, Page};
use rok_sqlc_api_app_axum::shared::pagination::{ListQuery, Pagination, SortOrder};
use rok_sqlc_api_app_axum::shared::response::PaginatedResponse;
use rok_sqlc_api_app_axum::shared::tenant::Tenant;
use rok_sqlc_api_app_axum::shared::validation::{Path, Query};
use serde_json::{Value, json};
use tower::ServiceExt;

/// `GET uri` on `app`: its status and body.
async fn get_body(app: Router, uri: &str, org: Option<&str>) -> (StatusCode, Vec<u8>) {
    let mut request = axum::http::Request::builder().uri(uri);
    if let Some(org) = org {
        request = request.header("x-org", org);
    }
    let request = request.body(Body::empty()).expect("a request");
    let response = app.oneshot(request).await.expect("an infallible router");
    let status = response.status();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("a body");
    (status, body.to_vec())
}

/// The query string `query` as the generated `Query` parses it, or its
/// error body.
async fn parse(query: &str) -> Result<ListQuery, Value> {
    let parsed = Arc::new(Mutex::new(None));
    let slot = Arc::clone(&parsed);
    let handler = move |Query(query): Query<ListQuery>| {
        let slot = Arc::clone(&slot);
        async move {
            *slot.lock().expect("an unpoisoned lock") = Some(query);
        }
    };
    let app = Router::new().route("/", get(handler));
    let (status, body) = get_body(app, &format!("/?{query}"), None).await;
    if status != StatusCode::OK {
        return Err(serde_json::from_slice(&body).expect("a JSON body"));
    }
    let query = parsed.lock().expect("an unpoisoned lock").take();
    Ok(query.expect("the parsed query"))
}

#[tokio::test]
async fn list_query_defaults_and_limits() {
    let query = parse("").await.expect("a valid list query");
    assert_eq!(
        query.pagination(),
        Pagination {
            page: 1,
            per_page: 20
        }
    );
    assert_eq!(query.sort_order(), SortOrder::Desc);

    let query = parse("page=3&per_page=500&q=ada&sort=title&order=asc")
        .await
        .expect("a valid list query");
    assert_eq!(
        query.pagination(),
        Pagination {
            page: 3,
            per_page: 100
        }
    );
    assert_eq!(query.q.as_deref(), Some("ada"));
    assert_eq!(query.sort.as_deref(), Some("title"));
    assert_eq!(query.sort_order(), SortOrder::Asc);

    let query = parse("page=0&per_page=0")
        .await
        .expect("a valid list query");
    assert_eq!(
        query.pagination(),
        Pagination {
            page: 1,
            per_page: 1
        }
    );
    let error = parse("order=sideways").await.expect_err("a bad order");
    assert_eq!(
        error,
        json!({"error": {
            "code": "bad_request",
            "message": "Query deserialize error: unknown variant `sideways`, expected `asc` or `desc`",
        }})
    );
}

#[test]
fn pagination_limit_and_offset() {
    let page = Pagination::new(Some(3), Some(25));
    assert_eq!(page.limit(), 25);
    assert_eq!(page.offset(), 50);
    assert_eq!(Pagination::default().offset(), 0);
    assert_eq!(Page::from(page), Page::new(25, 50));
    assert_eq!(Direction::from(SortOrder::Asc), Direction::Asc);
    assert_eq!(Direction::from(SortOrder::default()), Direction::Desc);
}

#[test]
fn paginated_response_body() {
    let response =
        PaginatedResponse::new(vec![1, 2], Pagination::new(Some(2), Some(2)), 5).map(|n| n * 10);
    assert_eq!(
        serde_json::to_value(&response).expect("serializes"),
        json!({"items": [10, 20], "page": 2, "per_page": 2, "total": 5})
    );
}

/// A path's parameters that don't parse are a `400` in actix-web's words,
/// whatever the path's shape: one value, a tuple, or named fields.
#[tokio::test]
async fn a_malformed_path_says_why_as_actix_web_does() {
    #[derive(serde::Deserialize)]
    struct Named {
        #[expect(dead_code, reason = "only parsed")]
        id: i64,
    }
    async fn one(Path(id): Path<i64>) -> String {
        id.to_string()
    }
    async fn pair(Path((id, _)): Path<(i64, uuid::Uuid)>) -> String {
        id.to_string()
    }
    async fn named(Path(_): Path<Named>) -> String {
        String::new()
    }
    let app = Router::new()
        .route("/one/{id}", get(one))
        .route("/pair/{id}/{key}", get(pair))
        .route("/named/{id}", get(named));
    for (uri, message) in [
        ("/one/7", None),
        ("/one/abc", Some("can not parse \"abc\" to a i64")),
        ("/pair/abc/x", Some("can not parse \"abc\" to a i64")),
        (
            "/pair/1/x",
            Some("UUID parsing failed: invalid character: found `x` at 0"),
        ),
        ("/named/abc", Some("can not parse \"abc\" to a i64")),
    ] {
        let (status, body) = get_body(app.clone(), uri, None).await;
        let Some(message) = message else {
            assert_eq!(status, StatusCode::OK, "{uri}");
            continue;
        };
        assert_eq!(status, StatusCode::BAD_REQUEST, "{uri}");
        let body: Value = serde_json::from_slice(&body).expect("a JSON body");
        assert_eq!(
            body["error"]["message"],
            format!("Path deserialize error: {message}"),
            "{uri}"
        );
    }
}

/// A path axum can't read at all is a wiring bug, not the caller's: a
/// route without the parameters its handler takes, or with more of them,
/// is a `500` with the generic message, its detail logged.
#[tokio::test]
async fn a_path_the_route_doesnt_provide_is_a_500() {
    async fn one(Path(id): Path<i64>) -> String {
        id.to_string()
    }
    let app = Router::new()
        .route("/none", get(one))
        .route("/two/{a}/{b}", get(one));
    for uri in ["/none", "/two/1/2"] {
        let (status, body) = get_body(app.clone(), uri, None).await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{uri}");
        let body: Value = serde_json::from_slice(&body).expect("a JSON body");
        assert_eq!(body["error"]["code"], "internal_error", "{uri}");
        assert_eq!(body["error"]["message"], "something went wrong", "{uri}");
    }
}

/// The tenant the test middleware stores.
#[derive(Debug, Clone, PartialEq)]
struct Org(String);

async fn whose(tenant: Tenant<Org>) -> String {
    tenant.into_inner().0
}

/// Stores the `x-org` header's value as the tenant, as an auth middleware
/// would.
async fn store_org(mut request: Request, next: Next) -> Response {
    let org = request
        .headers()
        .get("x-org")
        .and_then(|value| value.to_str().ok())
        .map(|org| Org(org.to_owned()));
    if let Some(org) = org {
        request.extensions_mut().insert(org);
    }
    next.run(request).await
}

#[tokio::test]
async fn tenant_comes_from_the_request_extensions() {
    let app = Router::new()
        .route("/", get(whose))
        .layer(middleware::from_fn(store_org));

    let (status, body) = get_body(app.clone(), "/", Some("acme")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, b"acme");

    // No middleware set one: a wiring bug, answered 500.
    let (status, body) = get_body(app, "/", None).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    let body: Value = serde_json::from_slice(&body).expect("a JSON body");
    assert_eq!(body["error"]["code"], "internal_error", "{body}");
}
