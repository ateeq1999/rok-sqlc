//! The OpenAPI paths of generated API features (ADR 0016, milestone 2):
//! every handler carries `#[utoipa::path]` with exactly the statuses it can
//! answer, the feature's tag and its handler's name, and `bearer_auth` when
//! it needs a caller; `generated.rs` holds `GeneratedApiDoc`. That the
//! document matches what the handlers do is tested in `tests/api-app`.

use std::fs;
use std::path::PathBuf;

use anyhow::Result;
use rok_sqlc::{ApiAccess, ApiConfig, Config, Derive, Extractor};
use tempfile::TempDir;

/// `accounts`: admin only, versioned (`revision`), with a unique `email`
/// (a `by-` route) and referenced by `notes`. `notes`: public, belonging to
/// an account (`?include=account`), soft-deleted. `tasks`: the default
/// access (`user`), owned, tenant-scoped.
const SCHEMA: &str = "CREATE TABLE accounts (
    id UUID PRIMARY KEY,
    email TEXT NOT NULL UNIQUE,
    revision INTEGER NOT NULL DEFAULT 1
);
CREATE TABLE notes (
    id BIGSERIAL PRIMARY KEY,
    account_id UUID NOT NULL REFERENCES accounts (id),
    title TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at TIMESTAMPTZ
);
CREATE TABLE tasks (
    id UUID PRIMARY KEY,
    tenant_id UUID NOT NULL,
    owner_id UUID NOT NULL,
    title TEXT NOT NULL
);
";

struct Project {
    dir: TempDir,
}

impl Project {
    /// The schema generated with `init`'s configuration, the conventions
    /// and the access above.
    fn generated() -> Result<Self> {
        Project::with_schema(SCHEMA)
    }

    /// `schema` generated as [`Project::generated`] generates its own.
    fn with_schema(schema: &str) -> Result<Self> {
        let project = Project {
            dir: TempDir::new()?,
        };
        fs::write(project.path("schema.sql"), schema)?;
        rok_sqlc::generate(&project.config())?;
        Ok(project)
    }

    fn path(&self, path: &str) -> PathBuf {
        self.dir.path().join(path)
    }

    fn read(&self, path: &str) -> Result<String> {
        Ok(fs::read_to_string(self.path(path))?)
    }

    fn config(&self) -> Config {
        let mut config = Config::default();
        config.schema = vec![self.path("schema.sql")];
        config.output = self.path("src/db");
        config.derive = vec![Derive::Serde, Derive::Validate];
        config.extractors = vec![Extractor::ActixWeb];
        config.conventions.soft_delete_column = Some("deleted_at".into());
        config.conventions.version_column = Some("revision".into());
        config.conventions.tenant_column = Some("tenant_id".into());
        let mut api = ApiConfig::new(self.path("src/features"));
        api.access.insert("accounts".into(), ApiAccess::Admin);
        api.access.insert("notes".into(), ApiAccess::Public);
        api.owner.insert("tasks".into(), "owner_id".into());
        config.api = Some(api);
        config
    }
}

/// The `#[utoipa::path(...)]` attribute right above `pub async fn <name>(`.
fn annotation<'a>(handlers: &'a str, name: &str) -> &'a str {
    let head = format!("pub async fn {name}(");
    let at = handlers
        .find(&head)
        .unwrap_or_else(|| panic!("no `{head}` in:\n{handlers}"));
    let start = handlers[..at]
        .rfind("#[utoipa::path(")
        .unwrap_or_else(|| panic!("`{name}` has no annotation:\n{handlers}"));
    let end = handlers[start..]
        .find(")]\n")
        .map_or(at, |index| start + index + 3);
    assert!(end <= at, "`{name}` has no annotation:\n{handlers}");
    &handlers[start..end]
}

/// The `status = …` codes an annotation lists, in order.
fn statuses(annotation: &str) -> Vec<u16> {
    annotation
        .split("status = ")
        .skip(1)
        .filter_map(|rest| rest.get(..3)?.parse().ok())
        .collect()
}

/// The list: method, absolute path, tag, operation id, the list query's
/// params, a page of responses, and `bearer_auth` for a signed-in caller.
#[test]
fn a_list_documents_its_route() -> Result<()> {
    let project = Project::generated()?;
    let handlers = project.read("src/features/tasks/handlers.rs")?;
    let list = annotation(&handlers, "list");
    for expected in [
        "#[utoipa::path(\n    get,\n    path = \"/api/v1/tasks\",\n    tag = \"tasks\",\n    operation_id = \"list\",\n",
        "    params(ListTasksQuery),\n",
        "(status = 200, description = \"One page of rows.\", body = PaginatedResponse<TaskResponse>)",
        "body = ErrorBody",
        "    security((\"bearer_auth\" = [])),\n",
    ] {
        assert!(list.contains(expected), "{expected}\n{list}");
    }
    // A bad query, and no caller.
    assert_eq!(statuses(list), [200, 400, 401]);
    // The tenant comes from middleware, so the operation doesn't show it.
    assert!(!list.contains("tenant"), "{list}");
    Ok(())
}

/// A versioned table's update takes `If-Match` (a `428` without it, a `412`
/// when stale) and its reads answer the `ETag`; an admin route answers `401`
/// and `403`; the create answers its `Location` and conflicts on the unique
/// `email`.
#[test]
fn versioned_admin_routes_document_etag_and_if_match() -> Result<()> {
    let project = Project::generated()?;
    let handlers = project.read("src/features/accounts/handlers.rs")?;
    let update = annotation(&handlers, "update");
    for expected in [
        "    path = \"/api/v1/accounts/{id}\",\n",
        "(\"id\" = uuid::Uuid, Path, description = \"Account id\")",
        "(\"If-Match\" = String, Header, description = \"The row's `ETag`.\")",
        "    request_body = UpdateAccountDto,\n",
        "(\"ETag\" = String, description = ",
        "    security((\"bearer_auth\" = [])),\n",
    ] {
        assert!(update.contains(expected), "{expected}\n{update}");
    }
    assert_eq!(
        statuses(update),
        [200, 400, 401, 403, 404, 409, 412, 413, 422, 428]
    );
    let get = annotation(&handlers, "get_one");
    assert!(get.contains("(\"ETag\" = String"), "{get}");
    assert_eq!(statuses(get), [200, 400, 401, 403, 404]);
    let create = annotation(&handlers, "create");
    assert!(create.contains("(\"Location\" = String"), "{create}");
    assert_eq!(statuses(create), [201, 400, 401, 403, 409, 413, 422]);
    // `notes` references accounts, so deleting one can conflict.
    assert_eq!(
        statuses(annotation(&handlers, "remove")),
        [204, 400, 401, 403, 404, 409]
    );
    // The `by-email` route: text never fails to parse.
    let lookup = annotation(&handlers, "get_by_email");
    assert!(
        lookup.contains("    path = \"/api/v1/accounts/by-email/{value}\",\n"),
        "{lookup}"
    );
    assert!(
        lookup.contains("(\"value\" = String, Path, description = \"Account email\")"),
        "{lookup}"
    );
    assert_eq!(statuses(lookup), [200, 401, 403, 404]);
    Ok(())
}

/// A public route carries no security, but `?include=` of an admin-only
/// relationship can need a caller, so its `GET /{id}` makes one optional.
/// A soft-delete table's `DELETE` takes `?permanent=`.
#[test]
fn public_routes_carry_no_security() -> Result<()> {
    let project = Project::generated()?;
    let handlers = project.read("src/features/notes/handlers.rs")?;
    let list = annotation(&handlers, "list");
    assert!(!list.contains("security"), "{list}");
    assert_eq!(statuses(list), [200, 400]);
    let get = annotation(&handlers, "get_one");
    assert!(
        get.contains("(\"id\" = i64, Path, description = \"Note id\")"),
        "{get}"
    );
    assert!(get.contains("        GetNoteQuery,\n"), "{get}");
    assert!(
        get.contains("    security((), (\"bearer_auth\" = [])),\n"),
        "{get}"
    );
    assert_eq!(statuses(get), [200, 400, 401, 403, 404]);
    let remove = annotation(&handlers, "remove");
    assert!(remove.contains("        RemoveNoteQuery,\n"), "{remove}");
    assert_eq!(statuses(remove), [204, 400, 404]);
    assert_eq!(statuses(annotation(&handlers, "restore")), [200, 400, 404]);
    // The has-many route from accounts takes the stricter access.
    let accounts = project.read("src/features/accounts/handlers.rs")?;
    let notes = annotation(&accounts, "list_notes");
    assert!(
        notes.contains("    path = \"/api/v1/accounts/{id}/notes\",\n"),
        "{notes}"
    );
    assert!(
        notes.contains("    operation_id = \"list_notes\",\n"),
        "{notes}"
    );
    assert!(notes.contains("body = Vec<NoteResponse>"), "{notes}");
    assert_eq!(statuses(notes), [200, 400, 401, 403, 404]);
    Ok(())
}

/// `generated.rs` holds `GeneratedApiDoc`: every generated path, health's
/// too, the sort keys and `?include=` names utoipa can't collect from the
/// params, a tag per feature, and the `bearer_auth` addon.
#[test]
fn generated_rs_holds_the_api_doc() -> Result<()> {
    let project = Project::generated()?;
    let generated = project.read("src/features/generated.rs")?;
    for expected in [
        "#[derive(utoipa::OpenApi)]\n#[openapi(\n    paths(\n",
        "        crate::shared::health::liveness,\n        crate::shared::health::readiness,\n",
        "        accounts::handlers::list,\n",
        "        accounts::handlers::get_by_email,\n",
        "        accounts::handlers::list_notes,\n",
        "        notes::handlers::restore,\n",
        "        tasks::handlers::remove,\n",
        "        crate::shared::pagination::SortOrder,\n",
        "        accounts::dto::AccountSort,\n",
        "        notes::dto::NoteInclude,\n",
        "    modifiers(&BearerAuth),\n",
        "        (name = \"health\"),\n",
        "        (name = \"accounts\"),\n",
        "pub struct GeneratedApiDoc;\n",
        "impl utoipa::Modify for BearerAuth {\n",
        ".bearer_format(\"JWT\")",
        "\"bearer_auth\"",
    ] {
        assert!(generated.contains(expected), "{expected}\n{generated}");
    }
    Ok(())
}

/// The shared health routes are documented under `health`, outside the
/// prefix, as the target document has them: `liveness` and `readiness`,
/// the handlers' names.
#[test]
fn health_routes_are_documented() -> Result<()> {
    let project = Project::generated()?;
    let health = project.read("src/shared/health.rs")?;
    for expected in [
        "#[utoipa::path(\n    get,\n    path = \"/health/live\",\n    tag = \"health\",\n    operation_id = \"liveness\",\n",
        "    responses((status = 200, description = \"The process is running\")),\n)]\npub async fn liveness() -> HttpResponse {",
        "    path = \"/health/ready\",\n    tag = \"health\",\n    operation_id = \"readiness\",\n",
        "(status = 200, description = \"The database is reachable\")",
        "(status = 503, description = \"The database is not reachable\", body = ErrorBody)",
        "pub async fn readiness(pool: web::Data<PgPool>) -> HttpResponse {",
        ".route(\"/health/live\", web::get().to(liveness))",
    ] {
        assert!(health.contains(expected), "{expected}\n{health}");
    }
    Ok(())
}

/// The data layer's `mod.rs` names Swagger UI beside utoipa, vendored so
/// its build needs no network.
#[test]
fn the_dependency_hints_name_swagger_ui() -> Result<()> {
    let project = Project::generated()?;
    let db_mod = project.read("src/db/mod.rs")?;
    assert!(
        db_mod.contains(
            "//   utoipa-swagger-ui = { version = \"9\", features = [\"actix-web\", \"vendored\"] }\n"
        ),
        "{db_mod}"
    );
    Ok(())
}

/// Path parameters are described as the target document describes them:
/// "<Singular> id" for a single key, and "<Singular> <column words>" for
/// each column of a composite key and for a unique lookup's value. A
/// many-to-many route's `{other}` names the related row by its id. (The
/// configuration needs the tables above.)
#[test]
fn path_parameters_are_described_like_the_target() -> Result<()> {
    let project = Project::with_schema(&format!(
        "{SCHEMA}CREATE TABLE posts (id BIGSERIAL PRIMARY KEY, title TEXT NOT NULL);
CREATE TABLE tags (id BIGSERIAL PRIMARY KEY, name TEXT NOT NULL UNIQUE);
CREATE TABLE post_tags (
    post_id BIGINT NOT NULL REFERENCES posts (id),
    tag_id BIGINT NOT NULL REFERENCES tags (id),
    PRIMARY KEY (post_id, tag_id)
);
CREATE TABLE stock_levels (
    warehouse_code TEXT NOT NULL,
    sku TEXT NOT NULL,
    quantity INTEGER NOT NULL,
    PRIMARY KEY (warehouse_code, sku)
);
",
    ))?;
    let posts = project.read("src/features/posts/handlers.rs")?;
    let get = annotation(&posts, "get_one");
    assert!(
        get.contains("(\"id\" = i64, Path, description = \"Post id\")"),
        "{get}"
    );
    let attach = annotation(&posts, "attach_tags");
    for expected in [
        "(\"id\" = i64, Path, description = \"Post id\")",
        "(\"other\" = i64, Path, description = \"Tag id\")",
    ] {
        assert!(attach.contains(expected), "{expected}\n{attach}");
    }
    let tags = project.read("src/features/tags/handlers.rs")?;
    let lookup = annotation(&tags, "get_by_name");
    assert!(
        lookup.contains("(\"value\" = String, Path, description = \"Tag name\")"),
        "{lookup}"
    );
    let levels = project.read("src/features/stock_levels/handlers.rs")?;
    let get = annotation(&levels, "get_one");
    for expected in [
        "(\"warehouse_code\" = String, Path, description = \"Stock level warehouse code\")",
        "(\"sku\" = String, Path, description = \"Stock level sku\")",
    ] {
        assert!(get.contains(expected), "{expected}\n{get}");
    }
    Ok(())
}
