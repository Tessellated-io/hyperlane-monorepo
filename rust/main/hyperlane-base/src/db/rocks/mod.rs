use std::{path::Path, sync::Arc};

use super::error::DbError;
use rocksdb::{Options, DB as Rocks};
use std::fs;
use std::io::Result as IoResult;
use std::os::unix::fs::PermissionsExt;
use tracing::{error, info};

pub use hyperlane_db::*;
pub use typed_db::*;

/// Shared functionality surrounding use of rocksdb
pub mod iterator;

/// DB operations tied to specific Mailbox
mod hyperlane_db;
/// Type-specific db operations
mod typed_db;

/// Database test utilities.
#[cfg(any(test, feature = "test-utils"))]
pub mod test_utils;

#[derive(Debug, Clone)]
/// A KV Store
pub struct DB(Arc<Rocks>);

impl From<Rocks> for DB {
    fn from(rocks: Rocks) -> Self {
        Self(Arc::new(rocks))
    }
}

type Result<T> = std::result::Result<T, DbError>;

// Thank you, ChatGPT
fn create_directory_if_not_exists(dir_path: &Path) -> IoResult<()> {
    // Check if the directory already exists
    if !dir_path.exists() {
        // Create the directory
        fs::create_dir(dir_path)?;

        // Set the directory permissions to 700
        let permissions = fs::Permissions::from_mode(0o700);
        fs::set_permissions(dir_path, permissions)?;

        info!(path = dir_path.to_str(), "created missing db folder");
    }

    Ok(())
}

impl DB {
    /// Opens db at `db_path` and creates if missing
    #[tracing::instrument(err)]
    pub fn from_path(db_path: &Path) -> Result<DB> {
        match create_directory_if_not_exists(db_path) {
            Ok(()) => {}
            Err(e) => error!(
                error = e.to_string(),
                folder = db_path.to_str(),
                "unable to create db folder"
            ),
        }

        let path = {
            let mut path = db_path
                .parent()
                .unwrap_or(Path::new("."))
                .canonicalize()
                .map_err(|e| DbError::InvalidDbPath(e, db_path.to_string_lossy().into()))?;
            if let Some(file_name) = db_path.file_name() {
                path.push(file_name);
            }
            path
        };

        if path.is_dir() {
            info!(path=%path.to_string_lossy(), "Opening existing db")
        } else {
            info!(path=%path.to_string_lossy(), "Creating db")
        }

        let mut opts = Options::default();
        opts.create_if_missing(true);

        Rocks::open(&opts, &path)
            .map_err(|e| DbError::OpeningError {
                source: e,
                path: db_path.into(),
                canonicalized: path,
            })
            .map(Into::into)
    }

    /// Store a value in the DB
    pub fn store(&self, key: &[u8], value: &[u8]) -> Result<()> {
        Ok(self.0.put(key, value)?)
    }

    /// Retrieve a value from the DB
    pub fn retrieve(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        Ok(self.0.get(key)?)
    }
}
