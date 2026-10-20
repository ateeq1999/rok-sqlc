//! The `[api]` section of a configuration source (ADR 0016).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::{ApiAccess, ApiConfig, ApiEndpoint, ApiFramework, ApiRule};

/// The `[api]` keys one source sets. Single keys merge one by one across
/// sources; a list replaces the lower list; table-keyed sections merge by
/// table, an upper entry replacing the lower one.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ApiKeys {
    output: Option<PathBuf>,
    db_path: Option<String>,
    prefix: Option<String>,
    framework: Option<FrameworkName>,
    search_endpoint: Option<bool>,
    openapi: Option<bool>,
    exclude: Option<Vec<String>>,
    #[serde(default)]
    skip: BTreeMap<String, Vec<EndpointName>>,
    #[serde(default)]
    routes: BTreeMap<String, String>,
    #[serde(default)]
    access: BTreeMap<String, AccessName>,
    #[serde(default)]
    owner: BTreeMap<String, String>,
    #[serde(default)]
    hidden: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    immutable: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    search: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    sort: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    rules: BTreeMap<String, BTreeMap<String, Vec<RuleSpec>>>,
}

/// A string rule of `[api.rules]`: a string, or an inline table with one
/// key, `regex`, `in`, `notIn`, `startsWith` or `endsWith`.
#[derive(Debug)]
struct RuleSpec(ApiRule);

/// What an inline rule table holds.
const INLINE_KEYS: &str =
    "an inline rule takes one key: `regex`, `in`, `notIn`, `startsWith` or `endsWith`";

impl<'de> Deserialize<'de> for RuleSpec {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = RuleSpec;

            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("a string rule, such as \"email\", or an inline table, such as { regex = \"^[a-z]+$\" }")
            }

            fn visit_str<E: serde::de::Error>(self, text: &str) -> Result<RuleSpec, E> {
                Ok(RuleSpec(ApiRule::Text(text.to_owned())))
            }

            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut map: A,
            ) -> Result<RuleSpec, A::Error> {
                use serde::de::Error;
                let Some(key) = map.next_key::<String>()? else {
                    return Err(A::Error::custom(INLINE_KEYS));
                };
                let rule = match key.as_str() {
                    "regex" => ApiRule::Regex(map.next_value()?),
                    "in" => ApiRule::In(map.next_value()?),
                    "notIn" => ApiRule::NotIn(map.next_value()?),
                    "startsWith" => ApiRule::StartsWith(map.next_value()?),
                    "endsWith" => ApiRule::EndsWith(map.next_value()?),
                    _ => return Err(A::Error::custom(format!("`{key}`: {INLINE_KEYS}"))),
                };
                if map.next_key::<String>()?.is_some() {
                    return Err(A::Error::custom(INLINE_KEYS));
                }
                Ok(RuleSpec(rule))
            }
        }
        deserializer.deserialize_any(Visitor)
    }
}

/// An `[api] framework` value. Deserializing an unknown name fails with the
/// list of known ones.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum FrameworkName {
    Actix,
    Axum,
}

impl FrameworkName {
    fn framework(self) -> ApiFramework {
        match self {
            FrameworkName::Actix => ApiFramework::Actix,
            FrameworkName::Axum => ApiFramework::Axum,
        }
    }
}

/// An `[api.access]` value. Deserializing an unknown name fails with the
/// list of known ones.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum AccessName {
    Public,
    User,
    Admin,
}

impl AccessName {
    fn access(self) -> ApiAccess {
        match self {
            AccessName::Public => ApiAccess::Public,
            AccessName::User => ApiAccess::User,
            AccessName::Admin => ApiAccess::Admin,
        }
    }
}

/// An `[api.skip]` entry. Deserializing an unknown name fails with the list
/// of known ones.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum EndpointName {
    List,
    GetOne,
    Create,
    Update,
    Remove,
    Search,
    Restore,
    Lookups,
    Relationships,
}

impl EndpointName {
    fn endpoint(self) -> ApiEndpoint {
        match self {
            EndpointName::List => ApiEndpoint::List,
            EndpointName::GetOne => ApiEndpoint::GetOne,
            EndpointName::Create => ApiEndpoint::Create,
            EndpointName::Update => ApiEndpoint::Update,
            EndpointName::Remove => ApiEndpoint::Remove,
            EndpointName::Search => ApiEndpoint::Search,
            EndpointName::Restore => ApiEndpoint::Restore,
            EndpointName::Lookups => ApiEndpoint::Lookups,
            EndpointName::Relationships => ApiEndpoint::Relationships,
        }
    }
}

impl ApiKeys {
    /// The keys `-a/--api-output` sets: `output`, or none.
    pub(crate) fn output(output: Option<PathBuf>) -> ApiKeys {
        ApiKeys {
            output,
            ..ApiKeys::default()
        }
    }

    /// The keys with `output`, as a file wrote it, resolved against the
    /// file's directory `base`.
    pub(super) fn relative_to(self, base: &Path) -> ApiKeys {
        ApiKeys {
            output: self.output.as_ref().map(|path| base.join(path)),
            ..self
        }
    }

    /// Layers `self` over `lower`, key by key and table by table.
    pub(super) fn over(self, lower: ApiKeys) -> ApiKeys {
        fn merge<V>(
            upper: BTreeMap<String, V>,
            mut lower: BTreeMap<String, V>,
        ) -> BTreeMap<String, V> {
            lower.extend(upper);
            lower
        }
        ApiKeys {
            output: self.output.or(lower.output),
            framework: self.framework.or(lower.framework),
            db_path: self.db_path.or(lower.db_path),
            prefix: self.prefix.or(lower.prefix),
            search_endpoint: self.search_endpoint.or(lower.search_endpoint),
            openapi: self.openapi.or(lower.openapi),
            exclude: self.exclude.or(lower.exclude),
            skip: merge(self.skip, lower.skip),
            routes: merge(self.routes, lower.routes),
            access: merge(self.access, lower.access),
            owner: merge(self.owner, lower.owner),
            hidden: merge(self.hidden, lower.hidden),
            immutable: merge(self.immutable, lower.immutable),
            search: merge(self.search, lower.search),
            sort: merge(self.sort, lower.sort),
            // By table, then by column: an upper column's rules replace the
            // lower ones.
            rules: {
                let mut rules = lower.rules;
                for (table, columns) in self.rules {
                    rules.entry(table).or_default().extend(columns);
                }
                rules
            },
        }
    }

    /// The API these keys describe: `None` without an `output`, which turns
    /// every other key off; otherwise the defaults overridden by each key
    /// set.
    pub(crate) fn resolve(self) -> Option<ApiConfig> {
        let mut api = ApiConfig::new(self.output?);
        if let Some(framework) = self.framework {
            api.framework = framework.framework();
        }
        if let Some(db_path) = self.db_path {
            api.db_path = db_path;
        }
        if let Some(prefix) = self.prefix {
            api.prefix = prefix;
        }
        api.search_endpoint = self.search_endpoint.unwrap_or_default();
        api.openapi = self.openapi.unwrap_or(api.openapi);
        api.exclude = self.exclude.unwrap_or_default();
        api.skip = self
            .skip
            .into_iter()
            .map(|(table, names)| {
                (
                    table,
                    names.into_iter().map(EndpointName::endpoint).collect(),
                )
            })
            .collect();
        api.routes = self.routes;
        api.access = self
            .access
            .into_iter()
            .map(|(table, name)| (table, name.access()))
            .collect();
        api.owner = self.owner;
        api.hidden = self.hidden;
        api.immutable = self.immutable;
        api.search = self.search;
        api.sort = self.sort;
        api.rules = self
            .rules
            .into_iter()
            .map(|(table, columns)| {
                let columns = columns
                    .into_iter()
                    .map(|(column, rules)| (column, rules.into_iter().map(|rule| rule.0).collect()))
                    .collect();
                (table, columns)
            })
            .collect();
        Some(api)
    }
}
