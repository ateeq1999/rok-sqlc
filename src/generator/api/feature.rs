//! The model every API feature renderer reads (ADR 0016): one [`Feature`]
//! per table with an API, holding its names, its route base, its key, and
//! the [`Endpoint`]s it serves, its unique [`Lookup`]s and its
//! [`Relation`]s. The renderers iterate them, so an endpoint added here
//! reaches the routes, handlers, and service trait in one place.

use std::collections::BTreeMap;

use heck::{ToKebabCase, ToUpperCamelCase};

use super::relations::Relation;
use super::scope::Caller;
use crate::generator::relationships::join_pair;
use crate::generator::search::{SearchApi, filter_relationships};
use crate::generator::write::{has_versioned_update, version_column};
use crate::schema::{BaseType, Column, FilterKind, Table};
use crate::{ApiAccess, ApiConfig, ApiEndpoint};

/// One endpoint of a feature, in the order the files list them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Endpoint {
    /// `GET /`: a page of rows.
    List,
    /// `POST /search`: a page of rows matching per-field filters.
    Search,
    /// `POST /`: create a row.
    Create,
    /// `GET /{key}`: one row.
    GetOne,
    /// `PATCH /{key}`: update a row.
    Update,
    /// `DELETE /{key}`: delete a row (soft-delete it, on a soft-delete
    /// table, which also takes `?permanent=true`).
    Remove,
    /// `POST /{key}/restore`: undo a soft delete.
    Restore,
}

impl Endpoint {
    /// Every endpoint, in order.
    const ALL: [Endpoint; 7] = [
        Endpoint::List,
        Endpoint::Search,
        Endpoint::Create,
        Endpoint::GetOne,
        Endpoint::Update,
        Endpoint::Remove,
        Endpoint::Restore,
    ];

    /// The handler's name, which is also the service method's.
    pub fn name(self) -> &'static str {
        match self {
            Endpoint::List => "list",
            Endpoint::Search => "search",
            Endpoint::Create => "create",
            Endpoint::GetOne => "get_one",
            Endpoint::Update => "update",
            Endpoint::Remove => "remove",
            Endpoint::Restore => "restore",
        }
    }

    /// The `[api.skip]` name of the endpoint.
    fn config(self) -> ApiEndpoint {
        match self {
            Endpoint::List => ApiEndpoint::List,
            Endpoint::Search => ApiEndpoint::Search,
            Endpoint::Create => ApiEndpoint::Create,
            Endpoint::GetOne => ApiEndpoint::GetOne,
            Endpoint::Update => ApiEndpoint::Update,
            Endpoint::Remove => ApiEndpoint::Remove,
            Endpoint::Restore => ApiEndpoint::Restore,
        }
    }

    /// The actix-web method guard, such as `get`.
    pub fn method(self) -> &'static str {
        match self {
            Endpoint::List | Endpoint::GetOne => "get",
            Endpoint::Create | Endpoint::Search | Endpoint::Restore => "post",
            Endpoint::Update => "patch",
            Endpoint::Remove => "delete",
        }
    }

    /// The HTTP method, such as `GET`.
    pub fn http_method(self) -> String {
        self.method().to_uppercase()
    }

    /// Whether the endpoint addresses one row by its key.
    pub fn takes_key(self) -> bool {
        matches!(
            self,
            Endpoint::GetOne | Endpoint::Update | Endpoint::Remove | Endpoint::Restore
        )
    }

    /// The path under the feature's URL, given the key's path.
    pub fn path(self, key_path: &str) -> String {
        match self {
            Endpoint::List | Endpoint::Create => String::new(),
            Endpoint::Search => "/search".to_owned(),
            Endpoint::GetOne | Endpoint::Update | Endpoint::Remove => key_path.to_owned(),
            Endpoint::Restore => format!("{key_path}/restore"),
        }
    }
}

/// A `GET /by-<column>/{value}` route: the data layer's lookup over a
/// single-column unique key that isn't hidden.
#[derive(Debug, Clone)]
pub struct Lookup<'a> {
    pub column: &'a Column,
    /// The data layer's function, which is also the handler's and the
    /// service method's name: `get_by_slug`.
    pub function: String,
}

impl Lookup<'_> {
    /// The route under the feature's URL: `/by-slug/{value}`.
    pub fn path(&self) -> String {
        format!("/by-{}/{{value}}", self.column.rust_name.to_kebab_case())
    }
}

/// How a feature's rows are addressed in the path.
#[derive(Debug, Clone)]
pub enum Key<'a> {
    /// One key column, taken as `{id}`.
    Single(&'a Column),
    /// Several, each taken as `{column}` in key order, read into the key
    /// struct.
    Composite(Vec<&'a Column>),
}

/// One table's API feature.
#[derive(Debug, Clone)]
pub struct Feature<'a> {
    pub table: &'a Table,
    /// The Rust path of the data layer, such as `crate::db`.
    pub db_path: &'a str,
    /// The feature's URL: the prefix and the route base, `/api/v1/notes`.
    pub url: String,
    /// The OpenAPI tag: the route base without its leading slash, `notes`.
    pub tag: String,
    pub key: Option<Key<'a>>,
    /// The endpoints served, in [`Endpoint::ALL`] order.
    pub endpoints: Vec<Endpoint>,
    /// The text columns `q` matches.
    pub search: Vec<&'a Column>,
    /// The columns `sort` takes, at least one when the list is served.
    /// Never a hidden column.
    pub sort: Vec<&'a Column>,
    /// Who may call the endpoints (`[api.access]`).
    pub access: ApiAccess,
    /// The owner column (`[api.owner]`): every query is scoped to the
    /// caller's id, and `create` sets it from the caller.
    pub owner: Option<&'a Column>,
    /// The columns left out of the response (`[api.hidden]`). They stay in
    /// the create and update bodies. Filters and sorts can't name them: a
    /// renderer taking either (the search body) checks
    /// [`Feature::is_hidden`].
    pub hidden: Vec<&'a Column>,
    /// The immutable columns (`[api.immutable]`): in the create body, and
    /// rejected by name in the update body. The data layer's patch keeps
    /// them. See [`Feature::is_immutable`].
    pub immutable: Vec<&'a Column>,
    /// The tenant column: every handler takes the `Tenant` extractor, and
    /// every data layer call the tenant.
    pub tenant: Option<&'a Column>,
    /// The version column: reads answer `ETag`, and `update` needs a
    /// matching `If-Match`.
    pub version: Option<&'a Column>,
    /// Whether rows are soft-deleted: `remove` takes `?permanent=true`.
    pub soft_delete: bool,
    /// The `GET /by-<column>/{value}` routes.
    pub lookups: Vec<Lookup<'a>>,
    /// The relationship routes, filled by [`super::features`] once every
    /// feature is known.
    pub relations: Vec<Relation<'a>>,
    /// Whether the data layer's filter can fail (it has relationship
    /// fields), so pushing its `WHERE` clause takes a `?`.
    pub filter_fallible: bool,
    /// Whether the types the feature exposes derive utoipa's `ToSchema` or
    /// `IntoParams` (`[api] openapi`).
    pub openapi: bool,
}

impl<'a> Feature<'a> {
    /// The feature of `table` (one of `tables`) under `api`, or `None` when
    /// the table has no API: it is excluded, or a join table. Its
    /// relations are left empty. Fails on a list with nothing to sort by.
    pub fn of(
        table: &'a Table,
        api: &'a ApiConfig,
        tables: &'a [Table],
    ) -> Result<Option<Self>, String> {
        let excluded = api.exclude.iter().any(|key| names(table, key));
        if excluded || join_pair(table).is_some() {
            return Ok(None);
        }
        let skipped = entry(&api.skip, table).cloned().unwrap_or_default();
        let key = match table.key_columns().as_slice() {
            [] => None,
            [column] => Some(Key::Single(column)),
            columns => Some(Key::Composite(columns.to_vec())),
        };
        let owner = entry(&api.owner, table)
            .and_then(|name| table.columns.iter().find(|column| column.sql_name == *name));
        let immutable = columns(table, entry(&api.immutable, table));
        // No update changes the owner or an immutable column.
        let has_patch = table.update_columns().iter().any(|column| {
            owner.is_none_or(|owner| owner.sql_name != column.sql_name)
                && !immutable.iter().any(|i| i.sql_name == column.sql_name)
        });
        let hidden = columns(table, entry(&api.hidden, table));
        let search = columns(table, entry(&api.search, table));
        let mut sort = columns(table, entry(&api.sort, table));
        if sort.is_empty() {
            sort = default_sort(table, &hidden);
        }
        let soft_delete = table.soft_delete_column.is_some();
        // The search body's filter is the data layer's, less hidden and
        // relationship fields; its sort keys are the list's.
        let searchable =
            api.search_endpoint && SearchApi::for_table(table).any() && !sort.is_empty();
        let endpoints: Vec<Endpoint> = Endpoint::ALL
            .into_iter()
            .filter(|endpoint| !skipped.contains(&endpoint.config()))
            .filter(|endpoint| match endpoint {
                Endpoint::List => true,
                Endpoint::Search => searchable,
                Endpoint::Create => !table.insert_columns().is_empty(),
                Endpoint::GetOne | Endpoint::Remove => key.is_some(),
                Endpoint::Update => key.is_some() && has_patch,
                Endpoint::Restore => key.is_some() && soft_delete,
            })
            .collect();
        if endpoints.contains(&Endpoint::List) && sort.is_empty() {
            return Err(format!(
                "`{}` has no column its list can sort by; name one in `api.sort`, or skip `list` in `api.skip`",
                table.display_name()
            ));
        }
        let lookups = if skipped.contains(&ApiEndpoint::Lookups) {
            Vec::new()
        } else {
            lookups(table, &hidden)
        };
        // A versioned table's `update` takes the version (`If-Match`).
        let version = version_column(table)
            .filter(|_| key.is_some())
            .filter(|_| !endpoints.contains(&Endpoint::Update) || has_versioned_update(table));
        let base = entry(&api.routes, table)
            .cloned()
            .unwrap_or_else(|| format!("/{}", table.module_name.to_kebab_case()));
        Ok(Some(Feature {
            table,
            db_path: &api.db_path,
            url: format!("{}{base}", api.prefix.trim_end_matches('/')),
            tag: base.trim_start_matches('/').to_owned(),
            key,
            endpoints,
            search,
            sort,
            access: entry(&api.access, table).copied().unwrap_or_default(),
            owner,
            hidden,
            immutable,
            tenant: table.tenant(),
            version,
            soft_delete,
            lookups,
            relations: Vec::new(),
            filter_fallible: !filter_relationships(table, tables).is_empty(),
            openapi: api.openapi,
        }))
    }

    /// The feature's relationship routes are skipped (`[api.skip]`).
    pub fn skips_relationships(api: &ApiConfig, table: &Table) -> bool {
        entry(&api.skip, table).is_some_and(|skip| skip.contains(&ApiEndpoint::Relationships))
    }

    /// Whether `column` is hidden: left out of the response, and never a
    /// filter or a sort key.
    pub fn is_hidden(&self, column: &Column) -> bool {
        self.hidden
            .iter()
            .any(|hidden| hidden.sql_name == column.sql_name)
    }

    /// Whether `column` is immutable: set on create, never by the update
    /// body.
    pub fn is_immutable(&self, column: &Column) -> bool {
        self.immutable
            .iter()
            .any(|immutable| immutable.sql_name == column.sql_name)
    }

    /// The columns the response holds: all but the hidden ones.
    pub fn response_columns(&self) -> Vec<&'a Column> {
        self.table
            .columns
            .iter()
            .filter(|column| !self.is_hidden(column))
            .collect()
    }

    /// Whether the feature reads one row the caller owns by its key: it has
    /// an owner and a key, so the data layer has `get_owned`, `lock_owned`
    /// and `owned_by` (which other features' relationship routes use too).
    pub fn reads_owned_rows(&self) -> bool {
        self.owner.is_some() && self.key.is_some()
    }

    /// The service methods' scope parameters: the tenant, then the caller
    /// when `user`.
    pub fn scope_params(&self, tenant: Option<&Column>, user: bool) -> Vec<String> {
        let mut params: Vec<String> = tenant
            .map(|tenant| format!("tenant: {}", tenant.value_type()))
            .into_iter()
            .collect();
        if user {
            params.push("user: AuthUser".to_owned());
        }
        params
    }

    /// Whether an endpoint's service method takes the caller (an owned
    /// feature's); a relation's may too ([`Relation::caller`]).
    pub fn takes_user(&self) -> bool {
        self.owner.is_some() && !(self.endpoints.is_empty() && self.lookups.is_empty())
    }

    /// Who the feature's own handlers act for: its access, and the caller's
    /// id when rows are owned.
    pub fn caller(&self) -> Caller {
        Caller {
            access: self.access,
            user: self.owner.is_some(),
        }
    }

    /// Whether the feature serves `endpoint`.
    pub fn serves(&self, endpoint: Endpoint) -> bool {
        self.endpoints.contains(&endpoint)
    }

    /// The module, such as `notes`.
    pub fn module(&self) -> &str {
        &self.table.module_name
    }

    /// The row struct, such as `Note`.
    pub fn row(&self) -> &str {
        &self.table.struct_name
    }

    /// The module in UpperCamelCase, such as `Notes`.
    fn plural(&self) -> String {
        self.table.module_name.to_upper_camel_case()
    }

    /// The service struct, such as `NotesService`.
    pub fn service(&self) -> String {
        format!("{}Service", self.plural())
    }

    /// The service trait, such as `NotesServiceApi`.
    pub fn service_api(&self) -> String {
        format!("{}ServiceApi", self.plural())
    }

    /// The create body, such as `CreateNoteDto`.
    pub fn create_dto(&self) -> String {
        format!("Create{}Dto", self.row())
    }

    /// The update body, such as `UpdateNoteDto`.
    pub fn update_dto(&self) -> String {
        format!("Update{}Dto", self.row())
    }

    /// The response body, such as `NoteResponse`.
    pub fn response(&self) -> String {
        format!("{}Response", self.row())
    }

    /// The list's query string, such as `ListNotesQuery`.
    pub fn list_query(&self) -> String {
        format!("List{}Query", self.plural())
    }

    /// The search body, such as `SearchNotesBody`.
    pub fn search_body(&self) -> String {
        format!("Search{}Body", self.plural())
    }

    /// The search body's filter, such as `NoteSearchFilter`.
    pub fn search_filter(&self) -> String {
        format!("{}SearchFilter", self.row())
    }

    /// The data layer's filter, such as `NoteFilter`.
    pub fn db_filter(&self) -> String {
        format!("{}Filter", self.row())
    }

    /// The query string of a soft-delete table's `DELETE`, such as
    /// `RemoveNoteQuery`.
    pub fn remove_query(&self) -> String {
        format!("Remove{}Query", self.row())
    }

    /// The query string of `GET /{key}` with `?include=`, such as
    /// `GetTaskQuery`.
    pub fn get_query(&self) -> String {
        format!("Get{}Query", self.row())
    }

    /// The relationships `?include=` names, such as `TaskInclude`.
    pub fn include_enum(&self) -> String {
        format!("{}Include", self.row())
    }

    /// The list's sort keys, such as `NoteSort`.
    pub fn sort_enum(&self) -> String {
        format!("{}Sort", self.row())
    }

    /// The data layer's module of the table, such as `crate::db::notes`.
    pub fn db_module(&self) -> String {
        format!("{}::{}", self.db_path, self.table.module_name)
    }

    /// The type the service takes the key as: the key column's (its typed
    /// id) or the key struct.
    pub fn key_type(&self) -> Option<String> {
        self.key
            .as_ref()
            .map(|_| crate::generator::keys::key_type(self.table))
    }

    /// The path of a row: `{id}`, or `{a}/{b}` for a composite key.
    pub fn key_path(&self) -> String {
        match &self.key {
            None => String::new(),
            Some(Key::Single(_)) => "/{id}".to_owned(),
            Some(Key::Composite(columns)) => columns
                .iter()
                .map(|column| format!("/{{{}}}", column.rust_name))
                .collect(),
        }
    }

    /// The default sort key: `created_at` when it is one, else the first.
    pub fn default_sort(&self) -> Option<&Column> {
        self.sort
            .iter()
            .find(|column| column.sql_name == "created_at")
            .or_else(|| self.sort.first())
            .copied()
    }
}

/// Whether the `[api]` key `key` names `table`: its schema-qualified name,
/// SQL name, or module name.
fn names(table: &Table, key: &str) -> bool {
    table.display_name() == key || table.sql_name == key || table.module_name == key
}

/// The entry of a per-table `[api]` map for `table`.
fn entry<'m, V>(map: &'m BTreeMap<String, V>, table: &Table) -> Option<&'m V> {
    map.iter()
        .find(|(key, _)| names(table, key))
        .map(|(_, value)| value)
}

/// The `by-` routes of `table`: its data layer lookups over one column,
/// other than a `hidden` one.
fn lookups<'a>(table: &'a Table, hidden: &[&Column]) -> Vec<Lookup<'a>> {
    crate::generator::read::unique_lookups(table)
        .into_iter()
        .filter_map(|lookup| match lookup.columns.as_slice() {
            [column] if !hidden.iter().any(|h| h.sql_name == column.sql_name) => Some(Lookup {
                column,
                function: lookup.fn_name,
            }),
            _ => None,
        })
        .collect()
}

/// The columns of `table` an `[api]` entry names, in its order.
fn columns<'a>(table: &'a Table, names: Option<&Vec<String>>) -> Vec<&'a Column> {
    names
        .into_iter()
        .flatten()
        .filter_map(|name| table.columns.iter().find(|column| column.sql_name == *name))
        .collect()
}

/// The sort keys of a table without `[api.sort]`: its key columns and
/// `created_at`, or, with neither, its first sortable column. Never a
/// `hidden` column (a key column can't be one).
fn default_sort<'a>(table: &'a Table, hidden: &[&Column]) -> Vec<&'a Column> {
    let sortable = |column: &&Column| {
        let visible = !hidden
            .iter()
            .any(|hidden| hidden.sql_name == column.sql_name);
        visible
            && (matches!(
                column.filter_kind(),
                Some(FilterKind::Ordered | FilterKind::Text)
            ) || column.base_type() == BaseType::Uuid)
    };
    let mut sort: Vec<&Column> = table.key_columns();
    sort.extend(
        table
            .columns
            .iter()
            .filter(|column| column.sql_name == "created_at" && !column.is_primary_key)
            .filter(sortable),
    );
    if sort.is_empty() {
        sort.extend(table.columns.iter().find(sortable));
    }
    sort
}
