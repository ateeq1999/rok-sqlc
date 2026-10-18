//! The model every API feature renderer reads (ADR 0016): one [`Feature`]
//! per table with an API, holding its names, its route base, its key, and
//! the [`Endpoint`]s it serves. The renderers iterate the endpoints, so an
//! endpoint added here (a lookup, a restore) reaches the routes, handlers,
//! and service trait in one place.

use std::collections::BTreeMap;

use heck::{ToKebabCase, ToUpperCamelCase};

use crate::generator::relationships::join_pair;
use crate::schema::{BaseType, Column, FilterKind, Table};
use crate::{ApiConfig, ApiEndpoint};

/// One endpoint of a feature, in the order the files list them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Endpoint {
    /// `GET /`: a page of rows.
    List,
    /// `POST /`: create a row.
    Create,
    /// `GET /{key}`: one row.
    GetOne,
    /// `PATCH /{key}`: update a row.
    Update,
    /// `DELETE /{key}`: delete a row.
    Remove,
}

impl Endpoint {
    /// Every endpoint, in order.
    const ALL: [Endpoint; 5] = [
        Endpoint::List,
        Endpoint::Create,
        Endpoint::GetOne,
        Endpoint::Update,
        Endpoint::Remove,
    ];

    /// The handler's name, which is also the service method's.
    pub fn name(self) -> &'static str {
        match self {
            Endpoint::List => "list",
            Endpoint::Create => "create",
            Endpoint::GetOne => "get_one",
            Endpoint::Update => "update",
            Endpoint::Remove => "remove",
        }
    }

    /// The `[api.skip]` name of the endpoint.
    fn config(self) -> ApiEndpoint {
        match self {
            Endpoint::List => ApiEndpoint::List,
            Endpoint::Create => ApiEndpoint::Create,
            Endpoint::GetOne => ApiEndpoint::GetOne,
            Endpoint::Update => ApiEndpoint::Update,
            Endpoint::Remove => ApiEndpoint::Remove,
        }
    }

    /// The actix-web method guard, such as `get`.
    pub fn method(self) -> &'static str {
        match self {
            Endpoint::List | Endpoint::GetOne => "get",
            Endpoint::Create => "post",
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
        matches!(self, Endpoint::GetOne | Endpoint::Update | Endpoint::Remove)
    }

    /// Whether the endpoint writes, so the service has an `_in` method for
    /// it running on the caller's connection.
    pub fn writes(self) -> bool {
        matches!(self, Endpoint::Create | Endpoint::Update | Endpoint::Remove)
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
    pub key: Option<Key<'a>>,
    /// The endpoints served, in [`Endpoint::ALL`] order.
    pub endpoints: Vec<Endpoint>,
    /// The text columns `q` matches.
    pub search: Vec<&'a Column>,
    /// The columns `sort` takes, at least one when the list is served.
    pub sort: Vec<&'a Column>,
}

impl<'a> Feature<'a> {
    /// The feature of `table` under `api`, or `None` when the table has no
    /// API: it is excluded, or a join table. Fails on a table the API can't
    /// serve yet.
    pub fn of(table: &'a Table, api: &'a ApiConfig) -> Result<Option<Self>, String> {
        let excluded = api.exclude.iter().any(|key| names(table, key));
        if excluded || join_pair(table).is_some() {
            return Ok(None);
        }
        let unsupported = [
            (&table.tenant_column, "a tenant column"),
            (&table.version_column, "a version column"),
        ];
        if let Some((_, what)) = unsupported.iter().find(|(column, _)| column.is_some()) {
            return Err(format!(
                "`{}` has {what}, which API features don't support yet; add it to `api.exclude`",
                table.display_name()
            ));
        }
        let skipped = entry(&api.skip, table).cloned().unwrap_or_default();
        let key = match table.key_columns().as_slice() {
            [] => None,
            [column] => Some(Key::Single(column)),
            columns => Some(Key::Composite(columns.to_vec())),
        };
        let has_patch = !table.update_columns().is_empty();
        let endpoints: Vec<Endpoint> = Endpoint::ALL
            .into_iter()
            .filter(|endpoint| !skipped.contains(&endpoint.config()))
            .filter(|endpoint| match endpoint {
                Endpoint::List => true,
                Endpoint::Create => !table.insert_columns().is_empty(),
                Endpoint::GetOne | Endpoint::Remove => key.is_some(),
                Endpoint::Update => key.is_some() && has_patch,
            })
            .collect();
        let search = columns(table, entry(&api.search, table));
        let mut sort = columns(table, entry(&api.sort, table));
        if sort.is_empty() {
            sort = default_sort(table);
        }
        if endpoints.contains(&Endpoint::List) && sort.is_empty() {
            return Err(format!(
                "`{}` has no column its list can sort by; name one in `api.sort`, or skip `list` in `api.skip`",
                table.display_name()
            ));
        }
        let base = entry(&api.routes, table)
            .cloned()
            .unwrap_or_else(|| format!("/{}", table.module_name.to_kebab_case()));
        Ok(Some(Feature {
            table,
            db_path: &api.db_path,
            url: format!("{}{base}", api.prefix.trim_end_matches('/')),
            key,
            endpoints,
            search,
            sort,
        }))
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

/// The columns of `table` an `[api]` entry names, in its order.
fn columns<'a>(table: &'a Table, names: Option<&Vec<String>>) -> Vec<&'a Column> {
    names
        .into_iter()
        .flatten()
        .filter_map(|name| table.columns.iter().find(|column| column.sql_name == *name))
        .collect()
}

/// The sort keys of a table without `[api.sort]`: its key columns and
/// `created_at`, or, with neither, its first sortable column.
fn default_sort(table: &Table) -> Vec<&Column> {
    let sortable = |column: &&Column| {
        matches!(
            column.filter_kind(),
            Some(FilterKind::Ordered | FilterKind::Text)
        ) || column.base_type() == BaseType::Uuid
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
