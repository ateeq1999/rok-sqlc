//! `primary_key_ids`: primary key values made by the generated code. The
//! PostgreSQL behavior of `uuid_v7` is proven in `tests/fixture-app`.

use super::*;

/// A UUID key, a text key, an integer key, and a composite key.
const ID_SCHEMA: &str = "CREATE TABLE events (id UUID PRIMARY KEY, name TEXT NOT NULL);
CREATE TABLE slugs (id TEXT PRIMARY KEY, label TEXT NOT NULL);
CREATE TABLE counters (id BIGSERIAL PRIMARY KEY, label TEXT NOT NULL);
CREATE TABLE pairs (a UUID NOT NULL, b UUID NOT NULL, PRIMARY KEY (a, b));
";

fn ids_project(strategy: &str) -> Result<TempDir> {
    let (dir, config) = project_with(
        ID_SCHEMA,
        &format!("[conventions]\nprimary_key_ids = \"{strategy}\"\n"),
    )?;
    generate(&config)?;
    Ok(dir)
}

#[test]
fn uuid_strategies_make_uuid_keys_optional_on_create() -> Result<()> {
    for (strategy, expr, feature) in [
        ("uuid_v4", "uuid::Uuid::new_v4()", "\"v4\""),
        ("uuid_v7", "uuid::Uuid::now_v7()", "\"v7\""),
    ] {
        let dir = ids_project(strategy)?;
        let events = models(&dir, "events")?;
        assert!(
            events.contains(&format!(
                "    pub fn generate() -> Self {{\n        Self({expr})\n    }}"
            )),
            "{events}"
        );
        assert!(events.contains("    pub id: Option<EventId>,"), "{events}");
        let write = resource_file(&dir, "events", "write.rs")?;
        assert!(
            write.contains(".bind(input.id.unwrap_or_else(EventId::generate))"),
            "{write}"
        );
        let db_mod = fs::read_to_string(dir.path().join("out/mod.rs"))?;
        assert!(
            db_mod.contains(&format!(
                "//   uuid = {{ version = \"1\", features = [{feature}] }}"
            )),
            "{db_mod}"
        );

        // Text, integer, and composite keys are left as they are.
        for module in ["slugs", "counters", "pairs"] {
            let source = models(&dir, module)?;
            assert!(!source.contains("fn generate"), "{module}: {source}");
        }
        assert!(models_field(&dir, "slugs", "id")?.contains("pub id: SlugId,"));
    }
    Ok(())
}

#[test]
fn ulid_makes_uuid_and_text_keys() -> Result<()> {
    let dir = ids_project("ulid")?;
    let events = models(&dir, "events")?;
    assert!(
        events.contains("        Self(uuid::Uuid::from(ulid::Ulid::new()))"),
        "{events}"
    );
    let slugs = models(&dir, "slugs")?;
    assert!(
        slugs.contains("        Self(ulid::Ulid::new().to_string())"),
        "{slugs}"
    );
    assert!(slugs.contains("    pub id: Option<SlugId>,"), "{slugs}");
    let write = resource_file(&dir, "slugs", "write.rs")?;
    assert!(
        write.contains(".bind(input.id.unwrap_or_else(SlugId::generate))"),
        "{write}"
    );
    let db_mod = fs::read_to_string(dir.path().join("out/mod.rs"))?;
    assert!(
        db_mod.contains("//   ulid = { version = \"1\", features = [\"uuid\"] }"),
        "{db_mod}"
    );
    Ok(())
}

#[test]
fn cuid2_makes_text_keys_and_leaves_uuid_keys_alone() -> Result<()> {
    let dir = ids_project("cuid2")?;
    let slugs = models(&dir, "slugs")?;
    assert!(
        slugs.contains("    pub fn generate() -> Self {\n        Self(cuid::cuid2())\n    }"),
        "{slugs}"
    );
    assert!(slugs.contains("    pub id: Option<SlugId>,"), "{slugs}");
    let write = resource_file(&dir, "slugs", "write.rs")?;
    assert!(
        write.contains(".bind(input.id.unwrap_or_else(SlugId::generate))"),
        "{write}"
    );
    let db_mod = fs::read_to_string(dir.path().join("out/mod.rs"))?;
    assert!(db_mod.contains("//   cuid = \"2\"\n"), "{db_mod}");

    // A UUID key is left as it is, as are integer and composite keys.
    for module in ["events", "counters", "pairs"] {
        let source = models(&dir, module)?;
        assert!(!source.contains("fn generate"), "{module}: {source}");
    }
    assert!(models_field(&dir, "events", "id")?.contains("pub id: EventId,"));
    Ok(())
}

#[test]
fn an_unknown_strategy_fails_the_run() -> Result<()> {
    let (_dir, config) = project_with(
        ID_SCHEMA,
        "[conventions]\nprimary_key_ids = \"snowflake\"\n",
    )?;
    let error = format!("{:#}", generate(&config).unwrap_err());
    assert!(error.contains("uuid_v7"), "{error}");
    Ok(())
}

fn models_field(dir: &TempDir, module: &str, field: &str) -> Result<String> {
    Ok(models(dir, module)?
        .lines()
        .filter(|line| line.contains(&format!("pub {field}:")))
        .collect::<Vec<_>>()
        .join("\n"))
}
