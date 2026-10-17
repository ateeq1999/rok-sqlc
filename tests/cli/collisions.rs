//! Generated names that collide fail generation.

use super::*;

/// The error a run on `schema` fails with; nothing is written.
fn generation_error(schema: &str) -> Result<String> {
    let dir = TempDir::new()?;
    let path = dir.path().join("schema.sql");
    fs::write(&path, schema)?;
    let err = run(&[
        "generate",
        "--schema",
        path.to_str().unwrap(),
        "--output",
        output_dir(&dir).to_str().unwrap(),
    ])
    .expect_err("generation should fail");
    assert!(!output_dir(&dir).exists());
    Ok(format!("{err:#}"))
}

// `user` and `users` both have the struct `User`, so both define `UserId`.

#[test]
fn two_imported_ids_with_one_name_fail_generation() -> Result<()> {
    let message = generation_error(
        "CREATE TABLE \"user\" (id BIGSERIAL PRIMARY KEY);
         CREATE TABLE users (id UUID PRIMARY KEY);
         CREATE TABLE posts (
             id BIGSERIAL PRIMARY KEY,
             editor_id BIGINT REFERENCES \"user\" (id),
             author_id UUID REFERENCES users (id)
         );",
    )?;
    assert!(
        message.contains(
            "`posts` would import two typed ids named `UserId`, from `user` and `users`; rename a module under `[modules]`"
        ),
        "{message}"
    );
    Ok(())
}

#[test]
fn an_id_colliding_with_an_imported_id_fails_generation() -> Result<()> {
    let message = generation_error(
        "CREATE TABLE \"user\" (id BIGSERIAL PRIMARY KEY);
         CREATE TABLE users (id UUID PRIMARY KEY, legacy_id BIGINT REFERENCES \"user\" (id));",
    )?;
    assert!(
        message.contains(
            "the typed id `UserId` imported from `user` collides with the typed id `UserId` that `users` generates; rename a module under `[modules]`"
        ),
        "{message}"
    );
    Ok(())
}

#[test]
fn an_imported_id_colliding_with_a_row_struct_fails_generation() -> Result<()> {
    // `x_ids` has the row struct `XId`, which is also the id of `x`.
    let message = generation_error(
        "CREATE TABLE x (id BIGSERIAL PRIMARY KEY);
         CREATE TABLE x_ids (id BIGSERIAL PRIMARY KEY, x_id BIGINT NOT NULL REFERENCES x (id));",
    )?;
    assert!(
        message.contains(
            "the typed id `XId` imported from `x` collides with the row struct `XId` that `x_ids` generates; rename a module under `[modules]`"
        ),
        "{message}"
    );
    Ok(())
}

/// Two tables whose struct names agree (`category` and `categories`) both
/// define `Category` and `CategoryFilter`; a resource whose files would
/// import both fails the run, even when neither is a typed id.
#[test]
fn a_file_importing_two_items_of_one_name_fails_generation() -> Result<()> {
    let dir = TempDir::new()?;
    let schema = dir.path().join("schema.sql");
    fs::write(
        &schema,
        "CREATE TABLE category (code TEXT NOT NULL UNIQUE, label TEXT NOT NULL);\n\
         CREATE TABLE categories (id BIGSERIAL PRIMARY KEY, name TEXT NOT NULL);\n\
         CREATE TABLE products (\n\
             id BIGSERIAL PRIMARY KEY,\n\
             category_code TEXT NOT NULL REFERENCES category (code),\n\
             category_id BIGINT NOT NULL REFERENCES categories (id)\n\
         );\n",
    )?;
    let out = output_dir(&dir);

    let output = run_binary(&[
        "g",
        "-s",
        schema.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--json",
        r#"{"relationships": {"products": {"category": "coded_category"}}}"#,
    ]);

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{stderr}");
    assert!(
        stderr.contains(
            "would import two items named `CategoryFilter`, from `categories` and `category`"
        ) || stderr
            .contains("would import two items named `Category`, from `categories` and `category`"),
        "{stderr}"
    );
    assert!(
        stderr.contains("`[modules]`") && stderr.contains("`[relationships]`"),
        "{stderr}"
    );
    assert!(!out.exists());
    Ok(())
}
