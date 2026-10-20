//! `rok-sqlc check-arch`: checks a project against ADR 0016's architecture.
//!
//! The rules and the layout it checks are stated once, in [`rules`]: the
//! lists of files each rule allows, and the texts the scaffolded `AGENTS.md`
//! and the architecture guide carry. The Rust source is read with `syn`, and
//! test code is exempt from every rule.

mod rules;
mod source;

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::ApiFramework;

pub(crate) use rules::rules_markdown;
use rules::{
    FRAMEWORK_FEATURE, FRAMEWORK_ROOT, SQLX_FEATURE, SQLX_PORT_FILES, SQLX_PORT_TYPES, SQLX_ROOT,
};
use source::Reference;

/// The files a feature directory may hold.
pub(crate) const FEATURE_FILES: &[&str] = &[
    "mod.rs",
    "routes.rs",
    "handlers.rs",
    "dto.rs",
    "model.rs",
    "repo.rs",
    "service_api.rs",
    "service.rs",
];

/// Module names no project may have.
const FORBIDDEN_NAMES: &[&str] = &["utils", "helpers", "common"];

/// What `check-arch` checks: a project's `src/`, and where its features and
/// data layer are.
#[derive(Debug)]
pub(crate) struct Project {
    /// The project directory; violations are shown relative to it.
    pub(crate) root: PathBuf,
    /// The features directory (`api.output`).
    pub(crate) features: PathBuf,
    /// The data layer's Rust path (`api.db_path`, `crate::db`).
    pub(crate) db_path: String,
    /// The web framework (`api.framework`) rule 1 names.
    pub(crate) framework: ApiFramework,
}

/// A rule `check-arch` enforces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Rule {
    Layout,
    Framework,
    OtherFramework,
    Sqlx,
    CrossFeature,
    SharedToFeatures,
    Panics,
    CrossFeatureWrite,
    CrossFeatureQuery,
}

impl Rule {
    /// Layout violations are warnings under `--lenient`.
    pub(crate) fn is_layout(self) -> bool {
        self == Rule::Layout
    }
}

impl fmt::Display for Rule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Rule::Layout => "layout",
            Rule::Framework | Rule::OtherFramework => "rule-1",
            Rule::Sqlx => "rule-2",
            Rule::CrossFeature => "rule-4",
            Rule::SharedToFeatures => "rule-5",
            Rule::Panics => "rule-6",
            Rule::CrossFeatureWrite => "cross-feature-write",
            Rule::CrossFeatureQuery => "cross-feature-query",
        })
    }
}

/// One violation, shown as `path:line: rule: message`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Violation {
    /// Relative to the project directory, `/`-separated.
    pub(crate) path: String,
    pub(crate) line: usize,
    pub(crate) rule: Rule,
    pub(crate) message: String,
}

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}: {}: {}",
            self.path, self.line, self.rule, self.message
        )
    }
}

/// Checks `project`, returning its violations by path and line.
pub(crate) fn check(project: &Project) -> Result<Vec<Violation>> {
    let src = project.root.join("src");
    if !src.is_dir() {
        bail!("no src/ directory in {}", shown(&project.root));
    }
    let layout = Layout::of(project, &src)?;
    let mut files = Vec::new();
    walk(&src, &mut files)?;

    let mut violations = Vec::new();
    for file in &files {
        let rel = components(file.strip_prefix(&src)?);
        let shown = format!("src/{}", rel.join("/"));
        let mut report = |line: usize, rule: Rule, message: String| {
            violations.push(Violation {
                path: shown.clone(),
                line,
                rule,
                message,
            });
        };
        layout.check_file(&rel, &mut report);
        if file.extension().is_some_and(|ext| ext == "rs") {
            let text =
                fs::read_to_string(file).with_context(|| format!("failed to read {shown}"))?;
            let parsed =
                syn::parse_file(&text).with_context(|| format!("failed to parse {shown}"))?;
            layout.check_source(&rel, &source::read(&parsed), &mut report);
        }
    }
    violations.sort();
    violations.dedup();
    Ok(violations)
}

/// Every file and directory under `dir`, sorted, directories before their
/// contents.
fn walk(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    let mut entries = fs::read_dir(dir)
        .with_context(|| format!("failed to read {}", shown(dir)))?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<std::io::Result<Vec<_>>>()?;
    entries.sort();
    for path in entries {
        out.push(path.clone());
        if path.is_dir() {
            walk(&path, out)?;
        }
    }
    Ok(())
}

fn components(path: &Path) -> Vec<String> {
    path.components()
        .map(|part| part.as_os_str().to_string_lossy().into_owned())
        .collect()
}

fn shown(path: &Path) -> String {
    path.display().to_string().replace('\\', "/")
}

/// Where things are in one project, relative to `src/`.
struct Layout {
    /// The features directory's components (and module path).
    features: Vec<String>,
    /// The data layer's module path.
    db: Vec<String>,
    /// The feature directories' names.
    feature_names: Vec<String>,
    /// The web framework rule 1 names.
    framework: ApiFramework,
}

/// What a path names in this crate, after `crate`, `self`, `super` and the
/// file's `use` bindings are resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Target {
    /// A module path from the crate root.
    Crate(Vec<String>),
    /// Anything else: another crate, the prelude, or a local name.
    Other(Vec<String>),
}

impl fmt::Display for Target {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Target::Crate(path) if path.is_empty() => f.write_str("crate"),
            Target::Crate(path) => write!(f, "crate::{}", path.join("::")),
            Target::Other(path) => f.write_str(&path.join("::")),
        }
    }
}

impl Layout {
    fn of(project: &Project, src: &Path) -> Result<Self> {
        let features = project.features.strip_prefix(src).with_context(|| {
            format!(
                "api.output ({}) is not under {}",
                shown(&project.features),
                shown(src)
            )
        })?;
        let features = components(features);
        let db = match project
            .db_path
            .split("::")
            .collect::<Vec<_>>()
            .split_first()
        {
            Some((&"crate", rest)) => rest.iter().map(|part| (*part).to_owned()).collect(),
            _ => bail!(
                "api.db_path (`{}`) does not start with `crate::`",
                project.db_path
            ),
        };
        let mut feature_names = Vec::new();
        if project.features.is_dir() {
            for entry in fs::read_dir(&project.features)? {
                let path = entry?.path();
                if path.is_dir() {
                    feature_names
                        .extend(path.file_name().map(|n| n.to_string_lossy().into_owned()));
                }
            }
        }
        Ok(Layout {
            features,
            db,
            feature_names,
            framework: project.framework,
        })
    }

    /// The feature `rel` (a path under `src/`) lies in, if any.
    fn feature_of<'a>(&self, rel: &'a [String]) -> Option<&'a str> {
        let rest = rel.strip_prefix(self.features.as_slice())?;
        (rest.len() > 1).then(|| rest[0].as_str())
    }

    fn in_shared(rel: &[String]) -> bool {
        rel.len() > 1 && rel[0] == "shared"
    }

    fn in_db(&self, rel: &[String]) -> bool {
        rel.len() > self.db.len() && rel.starts_with(&self.db)
    }

    /// Whether `rel` is the features' `generated.rs` (the generated wiring).
    fn is_generated_root(&self, rel: &[String]) -> bool {
        rel.strip_prefix(self.features.as_slice()) == Some(&["generated.rs".to_owned()][..])
    }

    fn is_root(rel: &[String], names: &[&str]) -> bool {
        rel.len() == 1 && names.contains(&rel[0].as_str())
    }

    /// The layout checks of one file or directory.
    fn check_file(&self, rel: &[String], report: &mut impl FnMut(usize, Rule, String)) {
        let Some(name) = rel.last() else {
            return;
        };
        let stem = name.strip_suffix(".rs").unwrap_or(name);
        if FORBIDDEN_NAMES.contains(&stem) {
            report(
                1,
                Rule::Layout,
                format!("no module may be named `{stem}`: put the code in shared/ or in a feature"),
            );
        }
        if let Some(rest) = rel.strip_prefix(self.features.as_slice())
            && rest.len() == 2
            && !FEATURE_FILES.contains(&rest[1].as_str())
        {
            report(
                1,
                Rule::Layout,
                format!(
                    "feature `{}` holds a file it may not: a feature has only {}",
                    rest[0],
                    FEATURE_FILES.join(", ")
                ),
            );
        }
        if rel.len() == 1 && !name.contains('.') {
            let mut allowed = vec!["shared", "bin"];
            allowed.extend(self.db.first().map(String::as_str));
            allowed.extend(self.features.first().map(String::as_str));
            if !allowed.contains(&name.as_str()) {
                allowed.sort_unstable();
                allowed.dedup();
                report(
                    1,
                    Rule::Layout,
                    format!(
                        "unknown top-level directory `{name}`: src/ holds only {}",
                        allowed.join(", ")
                    ),
                );
            }
        }
    }

    /// Rules 1, 2, 4, 5 and 6 and the cross-feature write and query rules, for the
    /// Rust file at `rel`.
    fn check_source(
        &self,
        rel: &[String],
        source: &source::Source,
        report: &mut impl FnMut(usize, Rule, String),
    ) {
        let module = module_path(rel);
        let bindings: Vec<(String, Target)> = source
            .references
            .iter()
            .filter_map(|reference| {
                let name = reference.binds.clone()?;
                Some((name, self.resolve(&module, &[], reference)))
            })
            .collect();

        for reference in &source.references {
            // A `use` leaf's own name is not yet bound while it resolves.
            let others: Vec<&(String, Target)> = bindings
                .iter()
                .filter(|(name, _)| reference.binds.as_deref() != Some(name.as_str()))
                .collect();
            let target = self.resolve(&module, &others, reference);
            for rule in self.violated(rel, &target) {
                // A path through a binding that breaks the rule itself is
                // reported once, at the `use`.
                let through = reference.segments.first().and_then(|first| {
                    others
                        .iter()
                        .find(|(name, _)| name == first)
                        .map(|(_, t)| t)
                });
                if !reference.leading_colon
                    && through.is_some_and(|via| self.violated(rel, via).contains(&rule))
                {
                    continue;
                }
                report(reference.line, rule, self.message(rel, rule, &target));
            }
        }
        for forbidden in &source.forbidden {
            report(
                forbidden.line,
                Rule::Panics,
                format!(
                    "`{}` outside tests: return an error instead",
                    forbidden.what
                ),
            );
        }
    }

    /// Resolves `reference`, written in the module `module`.
    fn resolve(
        &self,
        module: &[String],
        bindings: &[&(String, Target)],
        reference: &Reference,
    ) -> Target {
        let segments = &reference.segments;
        if reference.leading_colon {
            return Target::Other(segments.clone());
        }
        let Some(first) = segments.first() else {
            return Target::Other(Vec::new());
        };
        let (mut path, rest) = match first.as_str() {
            "crate" => (Vec::new(), &segments[1..]),
            "self" => (module.to_vec(), &segments[1..]),
            "super" => {
                let supers = segments.iter().take_while(|s| *s == "super").count();
                let keep = module.len().saturating_sub(supers);
                (module[..keep].to_vec(), &segments[supers..])
            }
            _ => match bindings.iter().find(|(name, _)| name == first) {
                Some((_, Target::Crate(path))) => (path.clone(), &segments[1..]),
                Some((_, Target::Other(path))) => {
                    let mut path = path.clone();
                    path.extend(segments[1..].iter().cloned());
                    return Target::Other(path);
                }
                None => return Target::Other(segments.clone()),
            },
        };
        path.extend(rest.iter().cloned());
        // Features are declared in `<features>/generated.rs` and
        // re-exported from the features module.
        if path.len() > self.features.len()
            && path.starts_with(&self.features)
            && path[self.features.len()] == "generated"
        {
            path.remove(self.features.len());
        }
        Target::Crate(path)
    }

    /// The rules `target`, named in the file at `rel`, breaks.
    fn violated(&self, rel: &[String], target: &Target) -> Vec<Rule> {
        let mut rules = Vec::new();
        let feature = self.feature_of(rel);
        let file = rel.last().map(String::as_str).unwrap_or_default();
        let infrastructure = Self::in_shared(rel) || self.in_db(rel) || self.is_generated_root(rel);
        match target {
            Target::Other(path) => match path.first().map(String::as_str) {
                Some(name) if name == self.framework.crate_name() => {
                    let allowed = infrastructure
                        || Self::is_root(rel, FRAMEWORK_ROOT)
                        || (feature.is_some() && FRAMEWORK_FEATURE.contains(&file));
                    if !allowed {
                        rules.push(Rule::Framework);
                    }
                }
                // The other framework's crate: only the data layer, whose
                // `extractors` may serve both, names it.
                Some(name) if name == self.framework.other().crate_name() => {
                    if !self.in_db(rel) {
                        rules.push(Rule::OtherFramework);
                    }
                }
                Some("sqlx") => {
                    let ports = feature.is_some()
                        && SQLX_PORT_FILES.contains(&file)
                        && path.len() == 2
                        && SQLX_PORT_TYPES.contains(&path[1].as_str());
                    let allowed = infrastructure
                        || ports
                        || Self::is_root(rel, SQLX_ROOT)
                        || (feature.is_some() && SQLX_FEATURE.contains(&file));
                    if !allowed {
                        rules.push(Rule::Sqlx);
                    }
                }
                _ => {}
            },
            Target::Crate(path) => {
                if let Some(own) = feature
                    && let Some(rest) = path.strip_prefix(self.features.as_slice())
                    && let Some(other) = rest.first()
                    && other != own
                    && self.feature_names.contains(other)
                {
                    rules.push(Rule::CrossFeature);
                }
                if Self::in_shared(rel) && path.starts_with(&self.features) {
                    rules.push(Rule::SharedToFeatures);
                }
                if let Some(own) = feature
                    && let Some(rest) = path.strip_prefix(self.db.as_slice())
                    && rest.len() >= 2
                    && rest[0] != own
                    && rest[1] == "write"
                {
                    rules.push(Rule::CrossFeatureWrite);
                }
                // A feature's own query module is `<db>::queries::<its
                // name>`; another feature's is reported.
                if let Some(own) = feature
                    && let Some(rest) = path.strip_prefix(self.db.as_slice())
                    && let [queries, module, ..] = rest
                    && queries == "queries"
                    && module != own
                    && self.feature_names.contains(module)
                {
                    rules.push(Rule::CrossFeatureQuery);
                }
            }
        }
        rules
    }

    fn message(&self, rel: &[String], rule: Rule, target: &Target) -> String {
        match rule {
            Rule::Framework => format!(
                "`{target}`: only {}, features/generated.rs, shared/ and db/ may use {}",
                [FRAMEWORK_FEATURE, FRAMEWORK_ROOT].concat().join(", "),
                self.framework.crate_name()
            ),
            Rule::OtherFramework => format!(
                "`{target}`: the project uses {} (`[api] framework`), so nothing but db/ may \
                 use {}",
                self.framework.name(),
                self.framework.other().crate_name()
            ),
            Rule::Sqlx => format!(
                "`{target}`: only {}, features/generated.rs, shared/ and db/ may use sqlx \
                 ({}: {} only)",
                [SQLX_FEATURE, SQLX_ROOT].concat().join(", "),
                SQLX_PORT_FILES.join(" and "),
                SQLX_PORT_TYPES.join(" and ")
            ),
            Rule::CrossFeature => format!(
                "`{target}`: feature `{}` refers to another feature; declare a port in its \
                 service.rs instead",
                self.feature_of(rel).unwrap_or_default()
            ),
            Rule::SharedToFeatures => format!("`{target}`: shared/ refers to the features"),
            Rule::CrossFeatureWrite => format!(
                "`{target}`: feature `{}` writes another table; declare a port in its \
                 service.rs and implement it in app.rs",
                self.feature_of(rel).unwrap_or_default()
            ),
            Rule::CrossFeatureQuery => format!(
                "`{target}`: feature `{}` calls another feature's query module; declare a port \
                 in its service.rs and implement it in app.rs",
                self.feature_of(rel).unwrap_or_default()
            ),
            Rule::Layout | Rule::Panics => String::new(),
        }
    }
}

/// The module path of the file at `rel` (under `src/`), going by where it
/// is: `features/notes/mod.rs` is `features::notes`.
fn module_path(rel: &[String]) -> Vec<String> {
    let Some((file, dirs)) = rel.split_last() else {
        return Vec::new();
    };
    let mut path = dirs.to_vec();
    match file.as_str() {
        "mod.rs" => {}
        "lib.rs" | "main.rs" if dirs.is_empty() => {}
        _ => path.push(file.strip_suffix(".rs").unwrap_or(file).to_owned()),
    }
    path
}
