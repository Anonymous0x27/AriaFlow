//! # AriaFlow Storage
//!
//! SQLite persistence for AriaFlow.
//!
//! - WAL journal mode, NORMAL synchronous, 5s busy timeout, 64 MiB cache
//! - Ordered, transactional migrations (`schema_migrations`)
//! - No module other than this crate touches the database
//!
//! Sensitive values (API token hashes, proxy passwords) are stored as-is for now;
//! hashing is applied at the API boundary, and field-level encryption is on the
//! roadmap (see docs/DATABASE.md).

use ariaflow_core::{AriaFlowError, Result};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;
use std::path::Path;
use std::str::FromStr;

pub mod repository;
pub mod schema;
pub mod service;

pub use repository::{DownloadRepository, HistoryRepository, SettingsRepository};
pub use service::SqliteDownloadService;

/// Handle to the SQLite database. Cheap to clone (connection pool inside).
#[derive(Clone)]
pub struct Database {
    pool: SqlitePool,
}

impl Database {
    /// Opens (creating if needed) the database at `path` and runs pending migrations.
    pub async fn connect(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let options = SqliteConnectOptions::from_str(&format!("sqlite:{}", path.display()))
            .map_err(|e| AriaFlowError::Storage(format!("invalid database url: {e}")))?
            .create_if_missing(true)
            .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
            .synchronous(sqlx::sqlite::SqliteSynchronous::Normal)
            .busy_timeout(std::time::Duration::from_secs(5))
            .foreign_keys(true)
            .pragma("cache_size", "-64000") // ~64 MiB
            .pragma("temp_store", "MEMORY");

        let pool = SqlitePoolOptions::new()
            .max_connections(8)
            .connect_with(options)
            .await
            .map_err(|e| AriaFlowError::Storage(format!("failed to open database: {e}")))?;

        let db = Self { pool };
        db.migrate().await?;
        Ok(db)
    }

    /// Opens an in-memory database (tests).
    pub async fn connect_in_memory() -> Result<Self> {
        let options = SqliteConnectOptions::from_str("sqlite::memory:")
            .map_err(|e| AriaFlowError::Storage(e.to_string()))?
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .map_err(|e| AriaFlowError::Storage(format!("failed to open memory db: {e}")))?;
        let db = Self { pool };
        db.migrate().await?;
        Ok(db)
    }

    async fn migrate(&self) -> Result<()> {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS schema_migrations (
                version    INTEGER PRIMARY KEY,
                name       TEXT NOT NULL,
                applied_at TEXT NOT NULL DEFAULT (datetime('now'))
            )",
        )
        .execute(&self.pool)
        .await
        .map_err(|e| AriaFlowError::Storage(format!("failed to create schema_migrations: {e}")))?;

        let applied: Vec<i64> = sqlx::query_scalar("SELECT version FROM schema_migrations")
            .fetch_all(&self.pool)
            .await
            .map_err(|e| {
                AriaFlowError::Storage(format!("failed to read schema_migrations: {e}"))
            })?;

        for (version, name, sql) in schema::MIGRATIONS {
            if applied.contains(version) {
                continue;
            }
            tracing::info!(version, name, "applying migration");
            let mut tx =
                self.pool.begin().await.map_err(|e| {
                    AriaFlowError::Storage(format!("migration tx begin failed: {e}"))
                })?;
            // sqlx cannot run multiple statements in one call, so split on ';' at top level.
            for stmt in sql.split(';') {
                let stmt = stmt.trim();
                if stmt.is_empty() {
                    continue;
                }
                sqlx::query(stmt).execute(&mut *tx).await.map_err(|e| {
                    AriaFlowError::Storage(format!(
                        "migration {version} ({name}) failed: {e}\nstatement: {stmt}"
                    ))
                })?;
            }
            sqlx::query("INSERT INTO schema_migrations(version, name) VALUES (?, ?)")
                .bind(version)
                .bind(name)
                .execute(&mut *tx)
                .await
                .map_err(|e| {
                    AriaFlowError::Storage(format!("migration bookkeeping failed: {e}"))
                })?;
            tx.commit()
                .await
                .map_err(|e| AriaFlowError::Storage(format!("migration commit failed: {e}")))?;
        }
        Ok(())
    }

    /// Hot backup of the database into `dest` using SQLite's `VACUUM INTO`.
    pub async fn backup(&self, dest: &Path) -> Result<()> {
        let dest_str = dest.to_string_lossy().replace('\'', "''");
        sqlx::query(&format!("VACUUM INTO '{dest_str}'"))
            .execute(&self.pool)
            .await
            .map_err(|e| AriaFlowError::Storage(format!("backup failed: {e}")))?;
        Ok(())
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }
}

#[cfg(test)]
mod migration_tests {
    //! Upgrade-path coverage: a database created by an older (v1-only) build
    //! must migrate cleanly to the current schema.
    use super::*;
    use sqlx::Row;

    #[tokio::test]
    async fn migration_v2_upgrades_a_populated_v1_database() {
        let dir = std::env::temp_dir().join(format!(
            "ariaflow-migration-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("ariaflow.db");

        {
            let options = SqliteConnectOptions::from_str(&format!("sqlite:{}", path.display()))
                .unwrap()
                .create_if_missing(true);
            let pool = SqlitePoolOptions::new()
                .max_connections(1)
                .connect_with(options)
                .await
                .unwrap();
            // Mirror of `Database::migrate`'s statement splitting, v1 only.
            sqlx::query(
                "CREATE TABLE IF NOT EXISTS schema_migrations (
                    version    INTEGER PRIMARY KEY,
                    name       TEXT NOT NULL,
                    applied_at TEXT NOT NULL DEFAULT (datetime('now'))
                )",
            )
            .execute(&pool)
            .await
            .unwrap();
            let (version, name, sql) = schema::MIGRATIONS[0];
            assert_eq!(version, 1);
            for stmt in sql.split(';') {
                let stmt = stmt.trim();
                if stmt.is_empty() {
                    continue;
                }
                sqlx::query(stmt).execute(&pool).await.unwrap();
            }
            sqlx::query("INSERT INTO schema_migrations(version, name) VALUES (1, ?)")
                .bind(name)
                .execute(&pool)
                .await
                .unwrap();
            // Minimal task row as an older build would have written it.
            sqlx::query(
                "INSERT INTO downloads(id, url, filename, save_path, created_at)
                 VALUES ('task-1', 'https://example.com/f.zip', 'f.zip', '/tmp/f.zip', datetime('now'))",
            )
            .execute(&pool)
            .await
            .unwrap();
            pool.close().await;
        }

        // Re-open with the current code: v2-v5 must be applied, v1 rows must survive.
        let db = Database::connect(&path).await.unwrap();
        let row = sqlx::query(
            "SELECT limit_rate_bytes_per_sec, max_retries, scheduled_start_at, auth_json, user_agent, ssh_certificate_id FROM downloads WHERE id = 'task-1'",
        )
        .fetch_one(db.pool())
        .await
        .unwrap();
        assert!(row
            .try_get::<Option<i64>, _>("limit_rate_bytes_per_sec")
            .unwrap()
            .is_none());
        assert!(row
            .try_get::<Option<i64>, _>("max_retries")
            .unwrap()
            .is_none());
        assert!(row
            .try_get::<Option<String>, _>("scheduled_start_at")
            .unwrap()
            .is_none());
        assert!(row
            .try_get::<Option<String>, _>("auth_json")
            .unwrap()
            .is_none());
        assert!(row
            .try_get::<Option<String>, _>("user_agent")
            .unwrap()
            .is_none());
        assert!(row
            .try_get::<Option<String>, _>("ssh_certificate_id")
            .unwrap()
            .is_none());
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM downloads WHERE id = 'task-1'")
            .fetch_one(db.pool())
            .await
            .unwrap();
        assert_eq!(count, 1);
        let applied: Vec<i64> = sqlx::query_scalar("SELECT version FROM schema_migrations")
            .fetch_all(db.pool())
            .await
            .unwrap();
        assert_eq!(applied, vec![1, 2, 3, 4, 5]);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
#[cfg(test)]
mod certificate_tests {
    //! CRUD + validation for `ssh_certificates`, plus dialect checks on the
    //! update semantics (keep/replace/clear passphrase).
    use super::*;
    use crate::repository::SshCertificateRepository;
    use ariaflow_core::{SshCertificateInput, SshCertificateInput as CertInput};

    const ED25519_UNENCRYPTED: &str = include_str!("../../../tests/fixtures/ssh_test_ed25519.pem");
    const ED25519_ENCRYPTED: &str =
        include_str!("../../../tests/fixtures/ssh_test_ed25519_enc.pem");

    async fn temp_db() -> (std::path::PathBuf, Database) {
        let dir = std::env::temp_dir().join(format!(
            "ariaflow-cert-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("ariaflow.db");
        let db = Database::connect(&path).await.unwrap();
        (dir, db)
    }

    #[tokio::test]
    async fn create_list_update_delete_roundtrip() {
        let (dir, db) = temp_db().await;
        let repo = SshCertificateRepository::new(db);

        // Create unencrypted key.
        let entry = repo
            .insert(&SshCertificateInput {
                name: "deploy-key".into(),
                private_key: Some(ED25519_UNENCRYPTED.into()),
                passphrase: None,
            })
            .await
            .expect("create unencrypted");
        assert_eq!(entry.algorithm, "ssh-ed25519");
        assert!(entry.fingerprint.starts_with("SHA256:"));
        assert!(!entry.has_passphrase);

        // List shows no material.
        let list = repo.list().await.unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, entry.id);

        // Material endpoint returns the key (engine use only).
        let material = repo.material(&entry.id).await.unwrap().expect("material");
        assert_eq!(material.0, "deploy-key");
        assert!(material.1.contains("PRIVATE KEY"));
        assert!(material.2.is_none());

        // Encrypted key without passphrase must fail.
        let err = repo
            .insert(&SshCertificateInput {
                name: "broken".into(),
                private_key: Some(ED25519_ENCRYPTED.into()),
                passphrase: None,
            })
            .await
            .unwrap_err();
        assert!(err.to_string().contains("passphrase"), "{err}");

        // Encrypted key with bogus passphrase must fail.
        let err = repo
            .insert(&SshCertificateInput {
                name: "broken2".into(),
                private_key: Some(ED25519_ENCRYPTED.into()),
                passphrase: Some("wrong-pass".into()),
            })
            .await
            .unwrap_err();
        assert!(err.to_string().contains("decrypt"), "{err}");

        // Encrypted key with the right passphrase works.
        let enc = repo
            .insert(&SshCertificateInput {
                name: "enc-key".into(),
                private_key: Some(ED25519_ENCRYPTED.into()),
                passphrase: Some("test-passphrase".into()),
            })
            .await
            .expect("create encrypted");
        assert!(enc.has_passphrase);

        // Update: rename only, key kept (private_key absent).
        let renamed = repo
            .update(
                &entry.id,
                &CertInput {
                    name: "deploy-key-renamed".into(),
                    private_key: None,
                    passphrase: None,
                },
            )
            .await
            .unwrap();
        assert_eq!(renamed.name, "deploy-key-renamed");
        assert_eq!(renamed.fingerprint, entry.fingerprint);

        // Update: clearing passphrase of an encrypted key must be refused.
        let err = repo
            .update(
                &enc.id,
                &CertInput {
                    name: "enc-key".into(),
                    private_key: None,
                    passphrase: Some(String::new()),
                },
            )
            .await
            .unwrap_err();
        assert!(err.to_string().contains("passphrase"), "{err}");

        // Update: unknown id → NotFound.
        let err = repo
            .update(
                "does-not-exist",
                &CertInput {
                    name: "x".into(),
                    private_key: None,
                    passphrase: None,
                },
            )
            .await
            .unwrap_err();
        assert!(matches!(err, AriaFlowError::NotFound(_)));

        // Delete roundtrip.
        repo.delete(&enc.id).await.unwrap();
        assert_eq!(repo.list().await.unwrap().len(), 1);
        assert!(repo.material(&enc.id).await.unwrap().is_none());
        let err = repo.delete(&enc.id).await.unwrap_err();
        assert!(matches!(err, AriaFlowError::NotFound(_)));

        drop(repo);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
