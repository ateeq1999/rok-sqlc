// Scaffolded once by rok-sqlc; this file is yours. It is never regenerated. See docs/guide/architecture.md.

//! `STORAGE_URL`: which store [`super::open`] opens.

use std::fmt;
use std::path::PathBuf;
use std::str::FromStr;

/// Where objects are stored, as `STORAGE_URL` names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StorageUrl {
    /// `file://<dir>`: files under a directory, created when missing.
    /// `file:///srv/files` (or `file:///C:/files`) is absolute,
    /// `file://./storage` relative to the working directory.
    File(PathBuf),
    /// `s3://<bucket>`: an S3-compatible bucket. The credentials, the
    /// region and the endpoint (`AWS_ENDPOINT_URL`, for MinIO and the
    /// like) come from the standard `AWS_*` variables.
    S3(String),
    /// `memory://`: kept in memory and lost on exit; for tests.
    Memory,
}

impl StorageUrl {
    /// What `STORAGE_URL` is when not set: `file://./storage`, a `storage`
    /// directory where the server runs (the project, under `cargo run`).
    #[must_use]
    pub fn default_for_development() -> Self {
        StorageUrl::File(PathBuf::from("./storage"))
    }
}

/// A `STORAGE_URL` that names no store.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("`{0}` is not a storage URL: use file://<dir>, s3://<bucket> or memory://")]
pub struct NotAStorageUrl(pub String);

impl FromStr for StorageUrl {
    type Err = NotAStorageUrl;

    fn from_str(url: &str) -> Result<Self, Self::Err> {
        let refused = || NotAStorageUrl(url.to_owned());
        if url == "memory://" {
            return Ok(StorageUrl::Memory);
        }
        if let Some(bucket) = url.strip_prefix("s3://") {
            let bucket = bucket.trim_end_matches('/');
            if bucket.is_empty() || bucket.contains('/') {
                return Err(refused());
            }
            return Ok(StorageUrl::S3(bucket.to_owned()));
        }
        let Some(path) = url.strip_prefix("file://") else {
            return Err(refused());
        };
        // `file:///C:/files`: a drive letter after the third slash.
        let path = match path.as_bytes() {
            [b'/', drive, b':', ..] if drive.is_ascii_alphabetic() => &path[1..],
            _ => path,
        };
        if path.is_empty() {
            return Err(refused());
        }
        Ok(StorageUrl::File(PathBuf::from(path)))
    }
}

impl fmt::Display for StorageUrl {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StorageUrl::File(path) => write!(formatter, "file://{}", path.display()),
            StorageUrl::S3(bucket) => write!(formatter, "s3://{bucket}"),
            StorageUrl::Memory => formatter.write_str("memory://"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(url: &str) -> Result<StorageUrl, NotAStorageUrl> {
        url.parse()
    }

    #[test]
    fn each_scheme_names_its_store() {
        assert_eq!(parse("memory://"), Ok(StorageUrl::Memory));
        assert_eq!(
            parse("s3://uploads"),
            Ok(StorageUrl::S3("uploads".to_owned()))
        );
        assert_eq!(
            parse("s3://uploads/"),
            Ok(StorageUrl::S3("uploads".to_owned()))
        );
        assert_eq!(
            parse("file:///srv/files"),
            Ok(StorageUrl::File(PathBuf::from("/srv/files")))
        );
        assert_eq!(
            parse("file:///C:/files"),
            Ok(StorageUrl::File(PathBuf::from("C:/files")))
        );
        assert_eq!(
            parse("file://./storage"),
            Ok(StorageUrl::File(PathBuf::from("./storage")))
        );
    }

    #[test]
    fn anything_else_is_refused() {
        for url in [
            "", "storage", "file://", "s3://", "s3://a/b", "http://x", "memory",
        ] {
            assert_eq!(parse(url), Err(NotAStorageUrl(url.to_owned())), "{url}");
        }
    }

    #[test]
    fn the_development_default_is_a_storage_directory() {
        assert_eq!(
            StorageUrl::default_for_development().to_string(),
            "file://./storage"
        );
    }
}
