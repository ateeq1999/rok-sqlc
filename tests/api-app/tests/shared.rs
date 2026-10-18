//! The generated shared tools: the list query and its page, the paginated
//! response, and the `Tenant` extractor. No database needed.

use actix_web::dev::Service;
use actix_web::http::StatusCode;
use actix_web::{App, HttpMessage, HttpResponse, test as actix_test, web};
use rok_sqlc_api_app::db::search::{Direction, Page};
use rok_sqlc_api_app::shared::pagination::{ListQuery, Pagination, SortOrder};
use rok_sqlc_api_app::shared::response::PaginatedResponse;
use rok_sqlc_api_app::shared::tenant::Tenant;
use serde_json::{Value, json};

/// The query string `query` as `web::Query` parses it.
fn parse(query: &str) -> ListQuery {
    let request = actix_test::TestRequest::with_uri(&format!("/?{query}")).to_http_request();
    web::Query::<ListQuery>::from_query(request.query_string())
        .expect("a valid list query")
        .into_inner()
}

#[test]
fn list_query_defaults_and_limits() {
    let query = parse("");
    assert_eq!(
        query.pagination(),
        Pagination {
            page: 1,
            per_page: 20
        }
    );
    assert_eq!(query.sort_order(), SortOrder::Desc);

    let query = parse("page=3&per_page=500&q=ada&sort=title&order=asc");
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

    assert_eq!(
        parse("page=0&per_page=0").pagination(),
        Pagination {
            page: 1,
            per_page: 1
        }
    );
    assert!(web::Query::<ListQuery>::from_query("order=sideways").is_err());
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

/// The tenant the test middleware stores.
#[derive(Debug, Clone, PartialEq)]
struct Org(String);

async fn whose(tenant: Tenant<Org>) -> HttpResponse {
    HttpResponse::Ok().body(tenant.into_inner().0)
}

#[actix_web::test]
async fn tenant_comes_from_the_request_extensions() {
    let app = actix_test::init_service(
        App::new()
            .wrap_fn(|req, srv| {
                if let Some(org) = req.headers().get("x-org").and_then(|v| v.to_str().ok()) {
                    req.extensions_mut().insert(Org(org.to_owned()));
                }
                srv.call(req)
            })
            .route("/", web::get().to(whose)),
    )
    .await;

    let request = actix_test::TestRequest::get()
        .uri("/")
        .insert_header(("x-org", "acme"))
        .to_request();
    let response = actix_test::call_service(&app, request).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(actix_test::read_body(response).await, "acme");

    // No middleware set one: a wiring bug, answered 500.
    let response =
        actix_test::call_service(&app, actix_test::TestRequest::get().uri("/").to_request()).await;
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let body: Value = actix_test::read_body_json(response).await;
    assert_eq!(body["error"]["code"], "internal_error", "{body}");
}
