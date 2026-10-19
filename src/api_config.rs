//! The `[api]` configuration (ADR 0016): where generated API features go and
//! how each table's API behaves. Checked against the schema by [`check`].

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

use heck::ToKebabCase;

use crate::Conventions;
use crate::api_rules::{self, ApiRule};
use crate::schema::{BaseType, Column, FilterKind, Table};

/// Generated API features for actix-web (ADR 0016), set by the config file's
/// `[api]` section ([`Config::api`](crate::Config::api)).
///
/// Every map is keyed by table: its SQL name, schema-qualified name, or
/// module name. Every table named must be generated, and every column named
/// must exist. New fields may be added in minor releases, so build one with
/// [`ApiConfig::new`] and assign the fields you need:
///
/// ```
/// use rok_sqlc::{ApiAccess, ApiConfig, ApiEndpoint};
/// let mut api = ApiConfig::new("src/features");
/// api.access.insert("users".into(), ApiAccess::Admin);
/// api.skip.insert("users".into(), vec![ApiEndpoint::Remove]);
/// let mut config = rok_sqlc::Config::default();
/// config.api = Some(api);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ApiConfig {
    /// The directory API features are generated into (`api_output`).
    pub output: PathBuf,
    /// The Rust path of the generated data layer. Defaults to `crate::db`.
    pub db_path: String,
    /// The URL prefix every feature's routes sit under. Defaults to
    /// `/api/v1`.
    pub prefix: String,
    /// Generate `POST /search` with per-field filters. Off by default.
    pub search_endpoint: bool,
    /// Describe the API for its OpenAPI document: utoipa's `ToSchema` or
    /// `IntoParams` on every type a feature exposes, and `ToSchema` on the
    /// data layer's types inside them. On by default; off, nothing
    /// generated mentions utoipa.
    pub openapi: bool,
    /// Tables to generate no API feature for.
    pub exclude: Vec<String>,
    /// Endpoints to leave out, by table.
    pub skip: BTreeMap<String, Vec<ApiEndpoint>>,
    /// Route bases, by table: each starts with `/` and is unique. A table
    /// without one is served under its kebab-case module name.
    pub routes: BTreeMap<String, String>,
    /// Who may call a table's endpoints. A table without an entry is
    /// [`ApiAccess::User`].
    pub access: BTreeMap<String, ApiAccess>,
    /// Owner columns, by table: a `UUID NOT NULL` column every query is
    /// scoped to the caller by.
    pub owner: BTreeMap<String, String>,
    /// Columns left out of responses, by table. A key column can't be hidden.
    pub hidden: BTreeMap<String, Vec<String>>,
    /// Immutable columns, by table: set on create, never changed by the
    /// generated update. Not a key, owner, tenant or managed column.
    pub immutable: BTreeMap<String, Vec<String>>,
    /// Text columns the list endpoint's `q` matches, by table.
    pub search: BTreeMap<String, Vec<String>>,
    /// Columns the list endpoint's `sort` takes, by table: ordered or text
    /// columns.
    pub sort: BTreeMap<String, Vec<String>>,
    /// String rules and transforms (VineJS names), by table, then by text
    /// column: checked or applied by the create and update bodies.
    pub rules: BTreeMap<String, BTreeMap<String, Vec<ApiRule>>>,
}

impl ApiConfig {
    /// The default `db_path`.
    pub const DEFAULT_DB_PATH: &str = "crate::db";
    /// The default `prefix`.
    pub const DEFAULT_PREFIX: &str = "/api/v1";

    /// API features generated into `output`, every other setting at its
    /// default.
    pub fn new(output: impl Into<PathBuf>) -> Self {
        Self {
            output: output.into(),
            db_path: Self::DEFAULT_DB_PATH.to_owned(),
            prefix: Self::DEFAULT_PREFIX.to_owned(),
            search_endpoint: false,
            openapi: true,
            exclude: Vec::new(),
            skip: BTreeMap::new(),
            routes: BTreeMap::new(),
            access: BTreeMap::new(),
            owner: BTreeMap::new(),
            hidden: BTreeMap::new(),
            immutable: BTreeMap::new(),
            search: BTreeMap::new(),
            sort: BTreeMap::new(),
            rules: BTreeMap::new(),
        }
    }
}

/// Who may call a table's API endpoints ([`ApiConfig::access`]).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ApiAccess {
    /// Anyone, without credentials.
    Public,
    /// Any signed-in user (`AuthUser`). The default.
    #[default]
    User,
    /// Administrators only (`AdminUser`).
    Admin,
}

impl ApiAccess {
    /// The name the config file and the JSON payload use.
    pub fn name(self) -> &'static str {
        match self {
            ApiAccess::Public => "public",
            ApiAccess::User => "user",
            ApiAccess::Admin => "admin",
        }
    }
}

/// One generated API endpoint, as [`ApiConfig::skip`] names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ApiEndpoint {
    /// `GET /`: a page of rows.
    List,
    /// `GET /{id}`: one row.
    GetOne,
    /// `POST /`: create a row.
    Create,
    /// `PATCH /{id}`: update a row.
    Update,
    /// `DELETE /{id}`: delete a row.
    Remove,
    /// `POST /search`: per-field filters.
    Search,
    /// `POST /{id}/restore`: undo a soft delete.
    Restore,
    /// `GET /by-<column>/{value}`: unique lookups.
    Lookups,
    /// Relationship routes.
    Relationships,
}

impl ApiEndpoint {
    /// The name the config file and the JSON payload use.
    pub fn name(self) -> &'static str {
        match self {
            ApiEndpoint::List => "list",
            ApiEndpoint::GetOne => "get_one",
            ApiEndpoint::Create => "create",
            ApiEndpoint::Update => "update",
            ApiEndpoint::Remove => "remove",
            ApiEndpoint::Search => "search",
            ApiEndpoint::Restore => "restore",
            ApiEndpoint::Lookups => "lookups",
            ApiEndpoint::Relationships => "relationships",
        }
    }
}

/// Where an `[api]` entry lives, as a dotted key path: `api.owner.posts`.
fn location(section: &str, table: &str) -> String {
    if table
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        format!("`api.{section}.{table}`")
    } else {
        format!("`api.{section}.\"{table}\"`")
    }
}

/// The table `key` names: its schema-qualified name, SQL name, or module
/// name.
fn find_table<'a>(tables: &'a [Table], key: &str) -> Option<&'a Table> {
    tables.iter().find(|table| {
        table.display_name() == key || table.module_name == key || table.sql_name == key
    })
}

/// Whether `a` and `b` are the same table, however their entries name it.
fn same(a: &Table, b: &Table) -> bool {
    a.display_name() == b.display_name()
}

/// The table an `[api.<section>]` entry names.
fn entry_table<'a>(tables: &'a [Table], section: &str, key: &str) -> Result<&'a Table, String> {
    find_table(tables, key).ok_or_else(|| {
        format!(
            "{} in the configuration names no generated table of the schema",
            location(section, key)
        )
    })
}

/// The column `name` of the table an `[api.<section>]` entry names.
fn entry_column<'a>(
    section: &str,
    key: &str,
    table: &'a Table,
    name: &str,
) -> Result<&'a Column, String> {
    table
        .columns
        .iter()
        .find(|column| column.sql_name == name)
        .ok_or_else(|| {
            format!(
                "{} in the configuration names `{name}`, which is not a column of `{}`",
                location(section, key),
                table.display_name()
            )
        })
}

/// Checks `api` against `tables`, the generated tables of the schema: every
/// table and column it names exists, owner, search and sort columns have
/// fitting types, an owned table isn't public, hidden columns aren't keys,
/// search columns or sort keys, immutable columns are ones an update could
/// change, string rules are well formed on text columns, and route bases
/// are well formed and unique.
pub(crate) fn check(
    api: &ApiConfig,
    tables: &[Table],
    conventions: &Conventions,
) -> Result<(), String> {
    let mut excluded = Vec::new();
    for key in &api.exclude {
        let table = find_table(tables, key).ok_or_else(|| {
            format!(
                "`api.exclude` in the configuration names `{key}`, which is no generated table of the schema"
            )
        })?;
        excluded.push(table);
    }
    for key in api.skip.keys() {
        entry_table(tables, "skip", key)?;
    }
    for key in api.access.keys() {
        entry_table(tables, "access", key)?;
    }
    for (key, name) in &api.owner {
        let table = entry_table(tables, "owner", key)?;
        let owner = entry_column("owner", key, table, name)?;
        if owner.nullable || owner.base_type() != BaseType::Uuid {
            return Err(format!(
                "{} in the configuration names `{name}`, which must be `UUID NOT NULL` to be an owner column",
                location("owner", key)
            ));
        }
        let access = api
            .access
            .iter()
            .find(|(other, _)| find_table(tables, other).is_some_and(|other| same(other, table)))
            .map(|(_, access)| *access)
            .unwrap_or_default();
        if access == ApiAccess::Public {
            return Err(format!(
                "{} in the configuration names `{name}`, but `{}` is public; an owned table needs a caller, so set {} to `user` or `admin`",
                location("owner", key),
                table.display_name(),
                location("access", key)
            ));
        }
    }
    // Hidden columns, by table: `q` and `sort` can't name one.
    let hidden = |table: &Table, name: &str| {
        api.hidden.iter().any(|(other, names)| {
            find_table(tables, other).is_some_and(|other| same(other, table))
                && names.iter().any(|hidden| hidden == name)
        })
    };
    let hidden_error = |section: &str, key: &str, table: &Table, name: &str, what: &str| {
        format!(
            "{} in the configuration names `{name}`, a hidden column of `{}`, which {what}",
            location(section, key),
            table.display_name()
        )
    };
    for (key, names) in &api.hidden {
        let table = entry_table(tables, "hidden", key)?;
        for name in names {
            if entry_column("hidden", key, table, name)?.is_primary_key {
                return Err(format!(
                    "{} in the configuration names `{name}`, a key column of `{}`, which can't be hidden",
                    location("hidden", key),
                    table.display_name()
                ));
            }
        }
    }
    for (key, names) in &api.search {
        let table = entry_table(tables, "search", key)?;
        for name in names {
            let kind = entry_column("search", key, table, name)?.filter_kind();
            if kind != Some(FilterKind::Text) {
                return Err(format!(
                    "{} in the configuration names `{name}`, which is not a text column, so `q` can't match it",
                    location("search", key)
                ));
            }
            if hidden(table, name) {
                return Err(hidden_error("search", key, table, name, "`q` can't match"));
            }
        }
    }
    for (key, names) in &api.sort {
        let table = entry_table(tables, "sort", key)?;
        for name in names {
            let kind = entry_column("sort", key, table, name)?.filter_kind();
            if !matches!(kind, Some(FilterKind::Ordered | FilterKind::Text)) {
                return Err(format!(
                    "{} in the configuration names `{name}`, which can't be sorted: sort columns are numbers, dates, times or text",
                    location("sort", key)
                ));
            }
            if hidden(table, name) {
                return Err(hidden_error(
                    "sort",
                    key,
                    table,
                    name,
                    "can't be a sort key",
                ));
            }
        }
    }
    check_immutable(api, tables, conventions)?;
    check_rules(api, tables, conventions)?;
    check_routes(api, tables, &excluded)
}

/// Each string rule names a text column generated code writes, and is well
/// formed.
fn check_rules(api: &ApiConfig, tables: &[Table], conventions: &Conventions) -> Result<(), String> {
    for (key, columns) in &api.rules {
        let table = entry_table(tables, "rules", key)?;
        for (name, rules) in columns {
            let column = entry_column("rules", key, table, name)?;
            let managed = conventions.managed_columns.iter().any(|m| m == name)
                || conventions.version_column.as_deref() == Some(name.as_str());
            if let Some(why) = api_rules::unfit(column, managed) {
                return Err(format!(
                    "{} in the configuration names `{name}`, {why}",
                    location("rules", key)
                ));
            }
            api_rules::parse_column(key, name, rules)?;
        }
    }
    Ok(())
}

/// Each immutable column exists and is one an update could change: not a
/// key, owner or tenant column (no update changes those anyway), nor a
/// managed or generated one (generated code never writes those).
fn check_immutable(
    api: &ApiConfig,
    tables: &[Table],
    conventions: &Conventions,
) -> Result<(), String> {
    for (key, names) in &api.immutable {
        let table = entry_table(tables, "immutable", key)?;
        let owner = api
            .owner
            .iter()
            .find(|(other, _)| find_table(tables, other).is_some_and(|other| same(other, table)))
            .map(|(_, owner)| owner.as_str());
        for name in names {
            let column = entry_column("immutable", key, table, name)?;
            let managed = conventions.managed_columns.iter().any(|m| m == name)
                || conventions.version_column.as_deref() == Some(name.as_str());
            let (what, then) = if column.is_primary_key {
                ("a key column", "no update changes anyway")
            } else if owner == Some(name.as_str()) {
                ("the owner column", "no update changes anyway")
            } else if conventions.tenant_column.as_deref() == Some(name.as_str()) {
                ("the tenant column", "no update changes anyway")
            } else if managed {
                ("a managed column", "generated code never writes")
            } else if column.is_generated {
                ("a generated column", "generated code never writes")
            } else {
                continue;
            };
            return Err(format!(
                "{} in the configuration names `{name}`, {what} of `{}`, which {then}",
                location("immutable", key),
                table.display_name()
            ));
        }
    }
    Ok(())
}

/// Each configured route base starts with `/` and is no other API
/// feature's route base, configured or default (`/` and the kebab-case
/// module name). Excluded tables have no route base.
fn check_routes(api: &ApiConfig, tables: &[Table], excluded: &[&Table]) -> Result<(), String> {
    // Configured route bases first, so a clash names one of them first.
    let mut bases: Vec<(String, String)> = Vec::new();
    let mut configured = Vec::new();
    for (key, base) in &api.routes {
        let table = entry_table(tables, "routes", key)?;
        if !base.starts_with('/') {
            return Err(format!(
                "{} in the configuration is `{base}`, but a route base must start with `/`",
                location("routes", key)
            ));
        }
        configured.push(table.module_name.as_str());
        bases.push((base.clone(), location("routes", key)));
    }
    for table in tables {
        let module = table.module_name.as_str();
        let has_api = !excluded.iter().any(|other| other.module_name == module);
        if has_api && !configured.contains(&module) {
            bases.push((
                format!("/{}", module.to_kebab_case()),
                format!("the default route base of `{}`", table.display_name()),
            ));
        }
    }
    let mut seen: HashMap<&str, &str> = HashMap::new();
    for (base, owner) in &bases {
        if let Some(first) = seen.insert(base, owner) {
            return Err(format!(
                "{first} in the configuration is `{base}`, the same route base as {owner}; route bases must be unique"
            ));
        }
    }
    Ok(())
}
