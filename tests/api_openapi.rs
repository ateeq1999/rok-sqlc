//! OpenAPI schemas for generated API features (ADR 0016, milestone 2):
//! with `api.output` set and `[api] openapi` on, every type a feature
//! exposes derives utoipa's `ToSchema` or `IntoParams`, and the data-layer
//! types inside them derive `ToSchema` too. Without the API, or with
//! `openapi = false`, nothing mentions utoipa. That the derives compile and
//! describe the wire format is tested in `tests/api-app`.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;
use rok_sqlc::{ApiConfig, Config, Derive, Extractor};
use tempfile::TempDir;

/// Typed ids (`notes.id`, and `tasks.note_id`, nullable), an enum type and a
/// value-list column, the foreign types (`chrono`, `rust_decimal`, JSON,
/// network types), a has-one relationship (`?include=`), a many-to-many
/// sync (`IdList`), and a soft-delete table (`?permanent=`).
const SCHEMA: &str = "CREATE TYPE mood AS ENUM ('happy', 'so-so');
CREATE TABLE notes (
    id UUID PRIMARY KEY,
    title TEXT NOT NULL,
    mood mood NOT NULL,
    size TEXT NOT NULL CHECK (size IN ('small', 'Large')),
    price NUMERIC(10, 2),
    specs JSONB,
    host INET,
    mac MACADDR NOT NULL,
    hosts INET[],
    made_on DATE NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at TIMESTAMPTZ
);
CREATE TABLE tasks (
    id BIGSERIAL PRIMARY KEY,
    note_id UUID UNIQUE REFERENCES notes (id),
    title TEXT NOT NULL
);
CREATE TABLE labels (id BIGSERIAL PRIMARY KEY, name TEXT NOT NULL);
CREATE TABLE note_labels (
    note_id UUID NOT NULL REFERENCES notes (id),
    label_id BIGINT NOT NULL REFERENCES labels (id),
    PRIMARY KEY (note_id, label_id)
);
";

struct Project {
    dir: TempDir,
}

impl Project {
    fn new() -> Result<Self> {
        let dir = TempDir::new()?;
        fs::write(dir.path().join("schema.sql"), SCHEMA)?;
        Ok(Project { dir })
    }

    fn path(&self, path: &str) -> PathBuf {
        self.dir.path().join(path)
    }

    fn read(&self, path: &str) -> Result<String> {
        Ok(fs::read_to_string(self.path(path))?)
    }

    /// `init`'s configuration with the soft-delete convention and
    /// `POST /search`, generating `src/db` and the features in
    /// `src/features`.
    fn config(&self) -> Config {
        let mut config = Config::default();
        config.schema = vec![self.path("schema.sql")];
        config.output = self.path("src/db");
        config.derive = vec![Derive::Serde, Derive::Validate];
        config.extractors = vec![Extractor::ActixWeb];
        config.conventions.soft_delete_column = Some("deleted_at".into());
        let mut api = ApiConfig::new(self.path("src/features"));
        api.search_endpoint = true;
        config.api = Some(api);
        config
    }
}

/// Every file under `dir`, by path relative to it.
fn read_tree(dir: &Path) -> Result<BTreeMap<PathBuf, String>> {
    let mut files = BTreeMap::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(next) = pending.pop() {
        for entry in fs::read_dir(&next)? {
            let path = entry?.path();
            if path.is_dir() {
                pending.push(path);
            } else {
                let relative = path.strip_prefix(dir)?.to_path_buf();
                files.insert(relative, fs::read_to_string(&path)?);
            }
        }
    }
    Ok(files)
}

/// The item `head` (`pub struct NoteResponse {`) starts, with the
/// attributes and doc lines above it, up to its closing line.
fn item<'a>(text: &'a str, head: &str) -> &'a str {
    let at = text
        .find(head)
        .unwrap_or_else(|| panic!("no `{head}` in:\n{text}"));
    let start = text[..at].rfind("\n\n").map_or(0, |index| index + 2);
    let end = text[at..]
        .find("\n}\n")
        .map_or(text.len(), |index| at + index + 3);
    &text[start..end]
}

#[test]
fn the_api_turns_openapi_on_by_default() {
    let api = ApiConfig::new("src/features");
    assert!(api.openapi);
}

/// The request bodies, responses and the search body's filter derive
/// `ToSchema`; the query strings derive `IntoParams`, in the query.
#[test]
fn every_dto_derives_its_schema() -> Result<()> {
    let project = Project::new()?;
    rok_sqlc::generate(&project.config())?;
    let dto = project.read("src/features/notes/dto.rs")?;
    for head in [
        "pub struct CreateNoteDto {",
        "pub struct UpdateNoteDto {",
        "pub struct NoteResponse {",
        "pub struct SearchNotesBody {",
        "pub struct NoteSearchFilter {",
    ] {
        let item = item(&dto, head);
        assert!(item.contains("utoipa::ToSchema"), "{item}");
    }
    let remove = item(&dto, "pub struct RemoveNoteQuery {");
    assert!(remove.contains("utoipa::IntoParams"), "{remove}");
    assert!(
        remove.contains("#[into_params(parameter_in = Query)]"),
        "{remove}"
    );

    let dto = project.read("src/features/tasks/dto.rs")?;
    let get = item(&dto, "pub struct GetTaskQuery {");
    assert!(get.contains("utoipa::IntoParams"), "{get}");
    assert!(
        get.contains("#[into_params(parameter_in = Query)]"),
        "{get}"
    );
    for head in [
        "pub enum TaskInclude {",
        "pub struct TaskWithNoteResponse {",
    ] {
        let item = item(&dto, head);
        assert!(item.contains("utoipa::ToSchema"), "{item}");
    }
    Ok(())
}

/// Typed ids show as their inner type, and the types utoipa can't describe
/// on its own get a `value_type`: network types are text. utoipa's own
/// features cover `chrono`, `rust_decimal`, UUIDs and JSON.
#[test]
fn typed_ids_and_foreign_types_get_value_types() -> Result<()> {
    let project = Project::new()?;
    rok_sqlc::generate(&project.config())?;
    let dto = project.read("src/features/notes/dto.rs")?;
    let response = item(&dto, "pub struct NoteResponse {");
    for expected in [
        "    #[schema(value_type = uuid::Uuid)]\n    pub id: NoteId,\n",
        "    #[schema(value_type = Option<String>)]\n    pub host: Option<ipnetwork::IpNetwork>,\n",
        "    #[schema(value_type = String)]\n    pub mac: mac_address::MacAddress,\n",
        "    #[schema(value_type = Option<Vec<String>>)]\n    pub hosts: Option<Vec<ipnetwork::IpNetwork>>,\n",
        // utoipa's features describe these.
        "    pub price: Option<rust_decimal::Decimal>,\n",
        "    pub specs: Option<serde_json::Value>,\n",
        "    pub made_on: chrono::NaiveDate,\n",
        "    pub mood: types::Mood,\n",
        "    pub size: types::NoteSize,\n",
    ] {
        assert!(response.contains(expected), "{expected}\n{response}");
    }
    for unexpected in [
        "pub price",
        "pub specs",
        "pub made_on",
        "pub mood",
        "pub size",
    ] {
        let line = response
            .lines()
            .position(|line| line.contains(unexpected))
            .expect("the field");
        let above = response.lines().nth(line - 1).unwrap_or_default();
        assert!(!above.contains("#[schema"), "{unexpected}: {above}");
    }

    // A nullable foreign key: `Option<NoteId>`, and in the update body the
    // patch's `Option<Option<NoteId>>`.
    let dto = project.read("src/features/tasks/dto.rs")?;
    let response = item(&dto, "pub struct TaskResponse {");
    assert!(
        response.contains(
            "    #[schema(value_type = Option<uuid::Uuid>)]\n    pub note_id: Option<NoteId>,\n"
        ),
        "{response}"
    );
    assert!(
        response.contains("    #[schema(value_type = i64)]\n    pub id: TaskId,\n"),
        "{response}"
    );
    let update = item(&dto, "pub struct UpdateTaskDto {");
    assert!(
        update.contains("    #[schema(value_type = Option<Option<uuid::Uuid>>)]\n"),
        "{update}"
    );
    Ok(())
}

/// A search condition's value must itself be a schema: a typed id, an enum
/// or a primitive is; any other value is described as text.
#[test]
fn search_conditions_over_foreign_types_are_text() -> Result<()> {
    let project = Project::new()?;
    rok_sqlc::generate(&project.config())?;
    let dto = project.read("src/features/notes/dto.rs")?;
    let filter = item(&dto, "pub struct NoteSearchFilter {");
    for expected in [
        "    pub id: Vec<Equality<NoteId>>,\n",
        "    pub mood: Vec<Equality<types::Mood>>,\n",
        "    pub title: Vec<Text>,\n",
        "    #[schema(value_type = Vec<Ordered<String>>)]\n    pub created_at: Vec<Ordered<chrono::DateTime<chrono::Utc>>>,\n",
        "    #[schema(value_type = Vec<Ordered<String>>)]\n    pub price: Vec<Ordered<rust_decimal::Decimal>>,\n",
        "    #[schema(value_type = Vec<Equality<String>>)]\n    pub host: Vec<Equality<ipnetwork::IpNetwork>>,\n",
    ] {
        assert!(filter.contains(expected), "{expected}\n{filter}");
    }
    Ok(())
}

/// The data layer's types inside the API's: typed ids, enums (keeping their
/// exact labels through serde's renames), sort keys, search conditions and
/// the error body.
#[test]
fn data_layer_types_the_api_exposes_derive_to_schema() -> Result<()> {
    let project = Project::new()?;
    rok_sqlc::generate(&project.config())?;
    let models = project.read("src/db/notes/models.rs")?;
    assert!(
        item(&models, "pub struct NoteId(").contains("utoipa::ToSchema"),
        "{models}"
    );
    let types = project.read("src/db/types.rs")?;
    for head in ["pub enum Mood {", "pub enum NoteSize {"] {
        let item = item(&types, head);
        assert!(item.contains("utoipa::ToSchema"), "{item}");
    }
    assert!(types.contains("#[serde(rename = \"so-so\")]"), "{types}");
    let listing = project.read("src/db/notes/listing.rs")?;
    assert!(
        item(&listing, "pub enum NoteSort {").contains("utoipa::ToSchema"),
        "{listing}"
    );
    let search = project.read("src/db/search.rs")?;
    for head in [
        "pub enum Ordered<T> {",
        "pub enum Equality<T> {",
        "pub enum Text {",
    ] {
        let item = item(&search, head);
        assert!(item.contains("utoipa::ToSchema"), "{item}");
    }
    // utoipa 5 can't describe a two-field variant over a generic type.
    assert!(
        item(&search, "pub enum Ordered<T> {")
            .contains("    #[schema(value_type = Value)]\n    Between(T, T),\n"),
        "{search}"
    );
    let http = project.read("src/db/http/mod.rs")?;
    for head in ["pub struct ErrorBody {", "pub struct ErrorPayload {"] {
        let item = item(&http, head);
        assert!(item.contains("ToSchema"), "{item}");
    }
    assert!(
        item(&http, "pub struct ErrorPayload {")
            .contains("    #[schema(value_type = Option<Object>)]\n    pub details: Option<ValidationErrors>,\n"),
        "{http}"
    );
    Ok(())
}

/// The shared tools' page, list query, sort order and many-to-many body.
#[test]
fn the_shared_tools_derive_their_schemas() -> Result<()> {
    let project = Project::new()?;
    rok_sqlc::generate(&project.config())?;
    let pagination = project.read("src/shared/pagination.rs")?;
    let query = item(&pagination, "pub struct ListQuery<");
    assert!(query.contains("utoipa::IntoParams"), "{query}");
    assert!(
        query.contains("#[into_params(parameter_in = Query)]"),
        "{query}"
    );
    assert!(
        item(&pagination, "pub enum SortOrder {").contains("utoipa::ToSchema"),
        "{pagination}"
    );
    let response = project.read("src/shared/response.rs")?;
    assert!(
        item(&response, "pub struct PaginatedResponse<T> {").contains("utoipa::ToSchema"),
        "{response}"
    );
    let validation = project.read("src/shared/validation.rs")?;
    assert!(
        item(&validation, "pub struct IdList<T> {").contains("utoipa::ToSchema"),
        "{validation}"
    );
    Ok(())
}

/// The data layer's `mod.rs` names utoipa with the features the schema
/// uses.
#[test]
fn the_dependency_hints_name_utoipa_with_the_features_used() -> Result<()> {
    let project = Project::new()?;
    rok_sqlc::generate(&project.config())?;
    let db_mod = project.read("src/db/mod.rs")?;
    assert!(
        db_mod.contains(
            "//   utoipa = { version = \"5\", features = [\"chrono\", \"decimal\", \"uuid\"] }\n"
        ),
        "{db_mod}"
    );

    // Without dates, decimals or UUIDs, no feature.
    fs::write(
        project.path("schema.sql"),
        "CREATE TABLE tags (id BIGSERIAL PRIMARY KEY, name TEXT NOT NULL);",
    )?;
    let mut config = project.config();
    config.force = true;
    rok_sqlc::generate(&config)?;
    let db_mod = project.read("src/db/mod.rs")?;
    assert!(db_mod.contains("//   utoipa = \"5\"\n"), "{db_mod}");
    Ok(())
}

/// With `openapi = false`, or without the API, no generated file mentions
/// utoipa.
#[test]
fn without_openapi_nothing_mentions_utoipa() -> Result<()> {
    let project = Project::new()?;
    let mut config = project.config();
    if let Some(api) = config.api.as_mut() {
        api.openapi = false;
    }
    rok_sqlc::generate(&config)?;
    let mut files = read_tree(&project.path("src"))?;
    assert!(files.len() > 20, "{:?}", files.keys());

    let plain = Project::new()?;
    let mut config = plain.config();
    config.api = None;
    rok_sqlc::generate(&config)?;
    files.extend(read_tree(&plain.path("src"))?);
    for (path, text) in &files {
        assert!(!text.contains("utoipa"), "{path:?}:\n{text}");
    }
    Ok(())
}

/// The milestone 1 record: `tests/openapi_off/milestone-1.txt`.
fn milestone_1_record() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/openapi_off/milestone-1.txt")
}

/// A 64-bit FNV-1a digest of `text`: enough to tell two versions of a
/// generated file apart, with no dependency.
fn digest(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// `[api] openapi = false` gives exactly milestone 1's output:
/// `tests/api-app`'s migrations and API settings
/// (`tests/openapi_off/rok-sqlc.toml`) generate the files
/// `tests/openapi_off/milestone-1.txt` records, one `<digest> <path>` line
/// each.
///
/// The record was made with milestone 1's binary (commit a685022) and
/// differs from it only where a later change was meant to reach output
/// without OpenAPI too: `db/pool.rs`'s doc comments (the 0.4 CI fixes) and
/// health's handlers, renamed `liveness` and `readiness` (milestone 2). To
/// rewrite it after another such change, run
/// `ROK_SQLC_UPDATE_SNAPSHOTS=1 cargo test --test api_openapi`, and diff
/// the output against that commit's binary to check nothing else moved.
#[test]
fn without_openapi_the_output_is_milestone_1s() -> Result<()> {
    let dir = TempDir::new()?;
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    fs::copy(
        source.join("openapi_off/rok-sqlc.toml"),
        dir.path().join("rok-sqlc.toml"),
    )?;
    fs::create_dir(dir.path().join("migrations"))?;
    for entry in fs::read_dir(source.join("api-app/migrations"))? {
        let path = entry?.path();
        let name = path.file_name().expect("a file name");
        fs::copy(&path, dir.path().join("migrations").join(name))?;
    }
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_rok-sqlc"))
        .arg("g")
        .current_dir(dir.path())
        .output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let mut record = String::new();
    for (path, text) in read_tree(&dir.path().join("src"))? {
        let path = path.to_string_lossy().replace('\\', "/");
        let text = text.replace("\r\n", "\n");
        record.push_str(&format!("{:016x} {path}\n", digest(&text)));
    }
    if std::env::var_os("ROK_SQLC_UPDATE_SNAPSHOTS").is_some() {
        fs::write(milestone_1_record(), &record)?;
        return Ok(());
    }
    let expected = fs::read_to_string(milestone_1_record())?.replace("\r\n", "\n");
    let differing: Vec<&str> = record
        .lines()
        .filter(|line| !expected.lines().any(|expected| expected == *line))
        .chain(
            expected
                .lines()
                .filter(|line| !record.lines().any(|actual| actual == *line)),
        )
        .collect();
    assert!(
        differing.is_empty(),
        "without OpenAPI, the output is no longer milestone 1's:\n{}",
        differing.join("\n")
    );
    Ok(())
}
