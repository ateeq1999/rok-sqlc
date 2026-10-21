//! `ALTER TABLE … DROP CONSTRAINT` and `ADD CONSTRAINT … CHECK` in later
//! migrations: the generated code is what a schema declared that way from
//! the start generates.

use super::*;

/// Generates from `migrations` (in order) and returns the generated files,
/// less the schema snapshot, which repeats the SQL.
fn generate(dir: &TempDir, name: &str, migrations: &[&str]) -> Result<BTreeMap<PathBuf, String>> {
    let input = dir.path().join(format!("{name}-migrations"));
    fs::create_dir_all(&input)?;
    for (index, sql) in migrations.iter().enumerate() {
        fs::write(input.join(format!("{:04}.sql", index + 1)), sql)?;
    }
    let out = dir.path().join(name);
    run(&[
        "generate",
        "-s",
        input.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
    ])?;
    let mut files = read_tree(&out)?;
    files.remove(Path::new("schema.rs"));
    Ok(files)
}

fn auth_tokens(purposes: &str) -> String {
    format!(
        "CREATE TABLE auth_tokens (
             id UUID PRIMARY KEY,
             purpose TEXT NOT NULL CHECK (purpose IN ({purposes}))
         );"
    )
}

#[test]
fn a_replaced_value_list_check_widens_the_generated_enum() -> Result<()> {
    let dir = TempDir::new()?;
    let widened = generate(
        &dir,
        "widened",
        &[
            &auth_tokens("'email_verification', 'password_reset'"),
            "ALTER TABLE auth_tokens DROP CONSTRAINT auth_tokens_purpose_check;
             ALTER TABLE auth_tokens ADD CONSTRAINT auth_tokens_purpose_check CHECK (
                 purpose IN ('email_verification', 'password_reset', 'two_factor_pending')
             );",
        ],
    )?;
    let declared = generate(
        &dir,
        "declared",
        &[&auth_tokens(
            "'email_verification', 'password_reset', 'two_factor_pending'",
        )],
    )?;
    assert!(
        widened[Path::new("types.rs")].contains("TwoFactorPending"),
        "{}",
        widened[Path::new("types.rs")]
    );
    assert_eq!(widened, declared);
    Ok(())
}

#[test]
fn a_dropped_unique_constraint_loses_its_lookup() -> Result<()> {
    let dir = TempDir::new()?;
    let dropped = generate(
        &dir,
        "dropped",
        &[
            "CREATE TABLE users (id UUID PRIMARY KEY, email TEXT NOT NULL UNIQUE);",
            "ALTER TABLE users DROP CONSTRAINT users_email_key;",
        ],
    )?;
    let kept = generate(
        &dir,
        "kept",
        &["CREATE TABLE users (id UUID PRIMARY KEY, email TEXT NOT NULL UNIQUE);"],
    )?;
    let never = generate(
        &dir,
        "never",
        &["CREATE TABLE users (id UUID PRIMARY KEY, email TEXT NOT NULL);"],
    )?;
    assert_ne!(kept, never, "a unique key generates a lookup");
    assert_eq!(dropped, never);
    Ok(())
}

#[test]
fn a_dropped_foreign_key_loses_its_relationship() -> Result<()> {
    let dir = TempDir::new()?;
    let users = "CREATE TABLE users (id UUID PRIMARY KEY);";
    let posts = |reference: &str| {
        format!("CREATE TABLE posts (id UUID PRIMARY KEY, author_id UUID NOT NULL{reference});")
    };
    let dropped = generate(
        &dir,
        "dropped",
        &[
            users,
            &posts(" REFERENCES users (id)"),
            "ALTER TABLE posts DROP CONSTRAINT IF EXISTS posts_author_id_fkey;",
        ],
    )?;
    let kept = generate(&dir, "kept", &[users, &posts(" REFERENCES users (id)")])?;
    let never = generate(&dir, "never", &[users, &posts("")])?;
    assert_ne!(kept, never, "a foreign key generates a relationship");
    assert_eq!(dropped, never);
    Ok(())
}

#[test]
fn dropping_an_unknown_constraint_fails_its_table_at_its_line() -> Result<()> {
    let dir = TempDir::new()?;
    let migrations = dir.path().join("migrations");
    fs::create_dir_all(&migrations)?;
    fs::write(migrations.join("0001_init.sql"), auth_tokens("'a', 'b'"))?;
    fs::write(
        migrations.join("0002_widen.sql"),
        "-- the inline check is auth_tokens_purpose_check\n\
         ALTER TABLE auth_tokens DROP CONSTRAINT auth_tokens_check;\n",
    )?;
    let out = output_dir(&dir);
    let output = run_binary(&[
        "g",
        "-s",
        migrations.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
    ]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{stderr}");
    let path = shown(&migrations.join("0002_widen.sql"));
    assert!(
        stderr.contains(&format!(
            "{path}:2: table `auth_tokens`: DROP CONSTRAINT: constraint `auth_tokens_check` does not exist"
        )),
        "{stderr}"
    );
    Ok(())
}
