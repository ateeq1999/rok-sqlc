//! Stale-file removal: dropped tables, hand-written files, filtered runs,
//! and kept query modules.

use super::*;

#[test]
fn regenerating_after_a_dropped_table_removes_its_resource_directory() -> Result<()> {
    let project = Project::new(TWO_TABLES)?;
    project.generate(&[])?;
    assert!(project.out().join("posts/models.rs").is_file());

    project.set_schema(USERS_ONLY)?;
    project.generate(&[])?;

    assert!(!project.out().join("posts").exists());
    assert!(project.out().join("users/models.rs").is_file());
    let db_mod = fs::read_to_string(project.out().join("mod.rs"))?;
    assert!(!db_mod.contains("pub mod posts;"));
    Ok(())
}

#[test]
fn removal_is_reported_and_dry_run_only_lists_it() -> Result<()> {
    let project = Project::new(TWO_TABLES)?;
    project.generate(&[])?;
    project.set_schema(USERS_ONLY)?;

    let dry = project.binary(&["--dry-run"]);
    assert!(dry.status.success());
    let posts_models = project.out().join("posts").join("models.rs");
    assert!(
        stdout(&dry).contains(&format!("would remove {}", shown(&posts_models))),
        "{}",
        stdout(&dry)
    );
    assert!(posts_models.is_file());

    let real = project.binary(&[]);
    assert!(real.status.success());
    assert!(stdout(&real).contains(&format!("removed {}", shown(&posts_models))));
    assert!(!posts_models.exists());
    Ok(())
}

#[test]
fn hand_written_files_survive_regeneration() -> Result<()> {
    let project = Project::new(TWO_TABLES)?;
    project.generate(&[])?;
    let notes = project.out().join("notes.rs");
    let helper = project.out().join("posts").join("helpers.rs");
    fs::write(&notes, "// hand-written\n")?;
    fs::write(&helper, "// hand-written helper\n")?;

    project.set_schema(USERS_ONLY)?;
    project.generate(&[])?;

    assert_eq!(fs::read_to_string(&notes)?, "// hand-written\n");
    assert_eq!(fs::read_to_string(&helper)?, "// hand-written helper\n");
    assert!(!project.out().join("posts/models.rs").exists());
    Ok(())
}

#[test]
fn filtered_runs_keep_other_generated_resources() -> Result<()> {
    let project = Project::new(TWO_TABLES)?;
    project.generate(&[])?;
    let before = project.tree()?;

    project.generate(&["--resource", "users"])?;

    assert_eq!(project.tree()?, before);
    Ok(())
}

#[test]
fn a_kept_queries_module_is_not_stale() -> Result<()> {
    let project = Project::new(USERS_ONLY)?;
    let queries = project.dir.path().join("queries.sql");
    fs::write(
        &queries,
        "-- name: GetUserByEmail :maybe_one\nSELECT id, email FROM users WHERE email = $1;\n",
    )?;
    project.generate(&["--queries", queries.to_str().unwrap()])?;

    project.generate(&[])?;

    assert!(project.out().join("queries.rs").is_file());
    Ok(())
}

/// Writes `queries/users.sql` and `queries/emails.sql` into the project and
/// returns the directory.
fn write_query_dir(project: &Project) -> Result<PathBuf> {
    let dir = project.dir.path().join("queries");
    fs::create_dir_all(&dir)?;
    fs::write(
        dir.join("users.sql"),
        "-- name: GetUserById :maybe_one\nSELECT id, email FROM users WHERE id = $1;\n",
    )?;
    fs::write(
        dir.join("emails.sql"),
        "-- name: GetUserByEmail :maybe_one\nSELECT id, email FROM users WHERE email = $1;\n",
    )?;
    Ok(dir)
}

#[test]
fn a_kept_queries_directory_is_not_stale() -> Result<()> {
    let project = Project::new(USERS_ONLY)?;
    let dir = write_query_dir(&project)?;
    project.generate(&["--queries", dir.to_str().unwrap()])?;
    let before = project.tree()?;

    project.generate(&[])?;

    assert_eq!(project.tree()?, before);
    Ok(())
}

#[test]
fn switching_from_a_query_file_to_a_directory_removes_queries_rs() -> Result<()> {
    let project = Project::new(USERS_ONLY)?;
    let file = project.dir.path().join("queries.sql");
    fs::write(
        &file,
        "-- name: GetUserByEmail :maybe_one\nSELECT id, email FROM users WHERE email = $1;\n",
    )?;
    project.generate(&["--queries", file.to_str().unwrap()])?;
    assert!(project.out().join("queries.rs").is_file());

    let dir = write_query_dir(&project)?;
    let output = project.binary(&["--queries", dir.to_str().unwrap()]);
    assert!(output.status.success(), "{output:?}");

    let out = project.out();
    assert!(!out.join("queries.rs").exists());
    assert!(out.join("queries/mod.rs").is_file());
    assert!(out.join("queries/users.rs").is_file());
    assert!(out.join("queries/emails.rs").is_file());
    let removed = out.join("queries.rs");
    assert!(
        stdout(&output).contains(&format!("removed {}", shown(&removed))),
        "{}",
        stdout(&output)
    );
    Ok(())
}

#[test]
fn switching_from_a_directory_to_a_query_file_removes_the_queries_directory() -> Result<()> {
    let project = Project::new(USERS_ONLY)?;
    let dir = write_query_dir(&project)?;
    project.generate(&["--queries", dir.to_str().unwrap()])?;
    assert!(project.out().join("queries/mod.rs").is_file());

    let file = dir.join("users.sql");
    project.generate(&["--queries", file.to_str().unwrap()])?;

    let out = project.out();
    assert!(out.join("queries.rs").is_file());
    assert!(!out.join("queries").exists());
    Ok(())
}

/// Generates from a migrations directory holding `files` (name, SQL) into
/// `<dir>/out`.
fn generate_migrations(dir: &Path, files: &[(&str, &str)]) -> Result<()> {
    let migrations = dir.join("migrations");
    fs::create_dir_all(&migrations)?;
    for (name, sql) in files {
        fs::write(migrations.join(name), sql)?;
    }
    rok_sqlc::cli::run_from([
        "rok-sqlc".to_owned(),
        "g".to_owned(),
        "-s".to_owned(),
        migrations.display().to_string(),
        "-o".to_owned(),
        dir.join("out").display().to_string(),
    ])
}

const BLOG: &str = "CREATE TABLE app.posts (id BIGSERIAL PRIMARY KEY, title TEXT NOT NULL);\n\
     CREATE TABLE comments (\n\
         id BIGSERIAL PRIMARY KEY,\n\
         post_id BIGINT NOT NULL REFERENCES app.posts (id),\n\
         body TEXT NOT NULL\n\
     );\n";

#[test]
fn a_table_dropped_by_a_later_migration_is_removed_with_its_files() -> Result<()> {
    let dir = TempDir::new()?;
    generate_migrations(dir.path(), &[("0001_init.up.sql", BLOG)])?;
    let out = dir.path().join("out");
    assert!(out.join("posts/models.rs").is_file());

    generate_migrations(
        dir.path(),
        &[(
            "0002_drop_posts.up.sql",
            "DROP TABLE IF EXISTS app.posts CASCADE;\nDROP TABLE IF EXISTS never_existed;\n",
        )],
    )?;

    assert!(!out.join("posts").exists(), "posts was not removed");
    let db_mod = fs::read_to_string(out.join("mod.rs"))?;
    assert!(!db_mod.contains("pub mod posts;"), "{db_mod}");
    // The foreign key went with the table (as with CASCADE): the column keeps
    // its raw type and no relationship points at the dropped table.
    let comments = fs::read_to_string(out.join("comments/models.rs"))?;
    assert!(comments.contains("pub post_id: i64,"), "{comments}");
    assert!(!out.join("comments/relationships.rs").exists());
    Ok(())
}

#[test]
fn a_table_can_be_recreated_after_it_was_dropped() -> Result<()> {
    let dir = TempDir::new()?;
    generate_migrations(
        dir.path(),
        &[
            ("0001_init.up.sql", BLOG),
            ("0002_drop.up.sql", "DROP TABLE comments;"),
            (
                "0003_recreate.up.sql",
                "CREATE TABLE comments (id BIGSERIAL PRIMARY KEY, text TEXT NOT NULL);",
            ),
        ],
    )?;
    let comments = fs::read_to_string(dir.path().join("out/comments/models.rs"))?;
    assert!(comments.contains("pub text: String,"), "{comments}");
    assert!(!comments.contains("post_id"), "{comments}");
    Ok(())
}
