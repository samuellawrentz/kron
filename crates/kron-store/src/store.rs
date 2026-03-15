use std::path::Path;

use rusqlite::{Connection, Row, params};
use rusqlite_migration::{M, Migrations};

use crate::error::StoreError;
use crate::models::{JobRecord, RunRecord, RunStatus};

const MIGRATIONS: &[M<'static>] = &[
    M::up(
        "CREATE TABLE IF NOT EXISTS jobs (
        id          TEXT PRIMARY KEY NOT NULL,
        name        TEXT UNIQUE NOT NULL,
        command     TEXT NOT NULL,
        schedule    TEXT NOT NULL,
        working_dir TEXT,
        created_at  TEXT NOT NULL,
        enabled     INTEGER NOT NULL DEFAULT 1
    );
    CREATE TABLE IF NOT EXISTS runs (
        id          TEXT PRIMARY KEY NOT NULL,
        job_id      TEXT NOT NULL DEFAULT '',
        job_name    TEXT NOT NULL,
        started_at  TEXT NOT NULL,
        finished_at TEXT,
        exit_code   INTEGER,
        stdout      TEXT NOT NULL DEFAULT '',
        stderr      TEXT NOT NULL DEFAULT '',
        status      TEXT NOT NULL DEFAULT 'running'
    );
    CREATE INDEX idx_runs_job_started ON runs(job_name, started_at DESC);",
    ),
    M::up(
        "CREATE TABLE jobs_new (
            id          TEXT PRIMARY KEY NOT NULL,
            name        TEXT,
            command     TEXT NOT NULL,
            schedule    TEXT NOT NULL,
            working_dir TEXT,
            created_at  TEXT NOT NULL,
            enabled     INTEGER NOT NULL DEFAULT 1
        );
        INSERT INTO jobs_new SELECT id, name, command, schedule, working_dir, created_at, enabled FROM jobs;
        DROP TABLE jobs;
        ALTER TABLE jobs_new RENAME TO jobs;
        CREATE INDEX IF NOT EXISTS idx_runs_job_id ON runs(job_id, started_at DESC);",
    ),
];

pub struct Store {
    conn: Connection,
}

fn job_from_row(row: &Row) -> rusqlite::Result<JobRecord> {
    let created_at_str: String = row.get(5)?;
    let enabled: i32 = row.get(6)?;
    let created_at = created_at_str
        .parse::<chrono::DateTime<chrono::Utc>>()
        .unwrap_or_else(|_| {
            tracing::warn!(value = %created_at_str, "failed to parse created_at, using epoch");
            chrono::DateTime::<chrono::Utc>::UNIX_EPOCH
        });
    Ok(JobRecord {
        id: row.get(0)?,
        name: row.get(1)?,
        command: row.get(2)?,
        schedule: row.get(3)?,
        working_dir: row.get(4)?,
        created_at,
        enabled: enabled != 0,
    })
}

fn run_from_row(row: &Row) -> rusqlite::Result<RunRecord> {
    let started_at_str: String = row.get(3)?;
    let finished_at_str: Option<String> = row.get(4)?;
    let status_str: String = row.get(8)?;

    let started_at = started_at_str
        .parse::<chrono::DateTime<chrono::Utc>>()
        .unwrap_or_else(|_| {
            tracing::warn!(value = %started_at_str, "failed to parse started_at, using epoch");
            chrono::DateTime::<chrono::Utc>::UNIX_EPOCH
        });
    let finished_at = finished_at_str.and_then(|s| {
        s.parse::<chrono::DateTime<chrono::Utc>>().ok().or_else(|| {
            tracing::warn!(value = %s, "failed to parse finished_at");
            None
        })
    });

    Ok(RunRecord {
        id: row.get(0)?,
        job_id: row.get(1)?,
        job_name: row.get(2)?,
        started_at,
        finished_at,
        exit_code: row.get(5)?,
        stdout: row.get(6)?,
        stderr: row.get(7)?,
        status: RunStatus::parse(&status_str),
    })
}

impl Store {
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                StoreError::Database(rusqlite::Error::SqliteFailure(
                    rusqlite::ffi::Error::new(14), // SQLITE_CANTOPEN
                    Some(format!("failed to create data directory: {e}")),
                ))
            })?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700)).map_err(
                    |e| {
                        StoreError::Database(rusqlite::Error::SqliteFailure(
                            rusqlite::ffi::Error::new(14),
                            Some(format!("failed to set data directory permissions: {e}")),
                        ))
                    },
                )?;
            }
        }
        let conn = Connection::open(path)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).map_err(
                |e| {
                    StoreError::Database(rusqlite::Error::SqliteFailure(
                        rusqlite::ffi::Error::new(14),
                        Some(format!("failed to set database file permissions: {e}")),
                    ))
                },
            )?;
        }
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys = ON;")?;
        let mut store = Self { conn };
        store.run_migrations()?;
        Ok(store)
    }

    pub fn open_in_memory() -> Result<Self, StoreError> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys = ON;")?;
        let mut store = Self { conn };
        store.run_migrations()?;
        Ok(store)
    }

    fn run_migrations(&mut self) -> Result<(), StoreError> {
        let migrations = Migrations::new(MIGRATIONS.to_vec());
        migrations.to_latest(&mut self.conn)?;
        Ok(())
    }

    pub fn insert_job(&self, job: &JobRecord) -> Result<(), StoreError> {
        let result = self.conn.execute(
            "INSERT INTO jobs (id, name, command, schedule, working_dir, created_at, enabled)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                job.id,
                job.name,
                job.command,
                job.schedule,
                job.working_dir,
                job.created_at.to_rfc3339(),
                i32::from(job.enabled),
            ],
        );

        match result {
            Ok(_) => Ok(()),
            Err(rusqlite::Error::SqliteFailure(e, _))
                if e.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                Err(StoreError::JobAlreadyExists {
                    name: job.name.clone().unwrap_or_else(|| job.id.clone()),
                })
            }
            Err(e) => Err(StoreError::Database(e)),
        }
    }

    pub fn list_jobs(&self) -> Result<Vec<JobRecord>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, command, schedule, working_dir, created_at, enabled
             FROM jobs
             ORDER BY name",
        )?;

        let rows = stmt.query_map([], job_from_row)?;

        let mut jobs = Vec::new();
        for row in rows {
            jobs.push(row?);
        }
        Ok(jobs)
    }

    /// Look up a job by its exact ID.
    pub fn get_job_by_id(&self, id: &str) -> Result<JobRecord, StoreError> {
        let result = self.conn.query_row(
            "SELECT id, name, command, schedule, working_dir, created_at, enabled
             FROM jobs WHERE id = ?1",
            params![id],
            job_from_row,
        );

        match result {
            Ok(job) => Ok(job),
            Err(rusqlite::Error::QueryReturnedNoRows) => Err(StoreError::JobNotFound {
                name: id.to_string(),
            }),
            Err(e) => Err(StoreError::Database(e)),
        }
    }

    /// Look up a job by ID first, then by name.
    pub fn get_job(&self, query: &str) -> Result<JobRecord, StoreError> {
        // Try by id first
        let by_id = self.conn.query_row(
            "SELECT id, name, command, schedule, working_dir, created_at, enabled
             FROM jobs WHERE id = ?1",
            params![query],
            job_from_row,
        );
        match by_id {
            Ok(job) => return Ok(job),
            Err(rusqlite::Error::QueryReturnedNoRows) => {}
            Err(e) => return Err(StoreError::Database(e)),
        }

        // Then try by name
        let by_name = self.conn.query_row(
            "SELECT id, name, command, schedule, working_dir, created_at, enabled
             FROM jobs WHERE name = ?1",
            params![query],
            job_from_row,
        );
        match by_name {
            Ok(job) => Ok(job),
            Err(rusqlite::Error::QueryReturnedNoRows) => Err(StoreError::JobNotFound {
                name: query.to_string(),
            }),
            Err(e) => Err(StoreError::Database(e)),
        }
    }

    /// Delete a job by its ID.
    pub fn delete_job(&self, id: &str) -> Result<(), StoreError> {
        let count = self
            .conn
            .execute("DELETE FROM jobs WHERE id = ?1", params![id])?;
        if count == 0 {
            return Err(StoreError::JobNotFound {
                name: id.to_string(),
            });
        }
        Ok(())
    }

    pub fn insert_run(&self, run: &RunRecord) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT INTO runs (id, job_id, job_name, started_at, finished_at, exit_code, stdout, stderr, status)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                run.id,
                run.job_id,
                run.job_name,
                run.started_at.to_rfc3339(),
                run.finished_at.map(|t| t.to_rfc3339()),
                run.exit_code,
                run.stdout,
                run.stderr,
                run.status.as_str(),
            ],
        )?;
        Ok(())
    }

    pub fn update_run(&self, run: &RunRecord) -> Result<(), StoreError> {
        let rows_affected = self.conn.execute(
            "UPDATE runs
             SET finished_at = ?1, exit_code = ?2, stdout = ?3, stderr = ?4, status = ?5
             WHERE id = ?6",
            params![
                run.finished_at.map(|t| t.to_rfc3339()),
                run.exit_code,
                run.stdout,
                run.stderr,
                run.status.as_str(),
                run.id,
            ],
        )?;
        if rows_affected == 0 {
            return Err(StoreError::RunNotFound { id: run.id.clone() });
        }
        Ok(())
    }

    /// List runs for a job. Queries by `job_id` OR `job_name` for backward compatibility
    /// with old runs that were stored with name-as-job_id.
    pub fn list_runs(&self, job_id: &str, limit: usize) -> Result<Vec<RunRecord>, StoreError> {
        let limit_i64 = i64::try_from(limit).unwrap_or(i64::MAX);
        let mut stmt = self.conn.prepare(
            "SELECT id, job_id, job_name, started_at, finished_at, exit_code, stdout, stderr, status
             FROM runs
             WHERE job_id = ?1 OR job_name = ?1
             ORDER BY started_at DESC
             LIMIT ?2",
        )?;

        let rows = stmt.query_map(params![job_id, limit_i64], run_from_row)?;

        let mut runs = Vec::new();
        for row in rows {
            runs.push(row?);
        }
        Ok(runs)
    }

    /// Count total and successful runs for a job.
    pub fn count_runs(&self, job_id: &str) -> Result<(u64, u64), StoreError> {
        let (total, success) = self.conn.query_row(
            "SELECT COUNT(*), COUNT(CASE WHEN status = 'success' THEN 1 END)
             FROM runs
             WHERE job_id = ?1 OR job_name = ?1",
            params![job_id],
            |row| Ok((row.get::<_, u64>(0)?, row.get::<_, u64>(1)?)),
        )?;
        Ok((success, total))
    }

    /// Get the most recent run across all jobs.
    pub fn get_latest_run(&self) -> Result<Option<RunRecord>, StoreError> {
        let result = self.conn.query_row(
            "SELECT id, job_id, job_name, started_at, finished_at, exit_code, stdout, stderr, status
             FROM runs
             ORDER BY started_at DESC
             LIMIT 1",
            [],
            run_from_row,
        );

        match result {
            Ok(run) => Ok(Some(run)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StoreError::Database(e)),
        }
    }

    /// Get the most recent run for a job. Queries by `job_id` OR `job_name` for backward compat.
    pub fn get_last_run(&self, job_id: &str) -> Result<Option<RunRecord>, StoreError> {
        let result = self.conn.query_row(
            "SELECT id, job_id, job_name, started_at, finished_at, exit_code, stdout, stderr, status
             FROM runs
             WHERE job_id = ?1 OR job_name = ?1
             ORDER BY started_at DESC
             LIMIT 1",
            params![job_id],
            run_from_row,
        );

        match result {
            Ok(run) => Ok(Some(run)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StoreError::Database(e)),
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::similar_names)]
mod tests {
    use chrono::Utc;

    use super::*;

    fn make_job(id: &str, name: Option<&str>) -> JobRecord {
        JobRecord {
            id: id.to_string(),
            name: name.map(str::to_string),
            command: "echo hello".to_string(),
            schedule: "0 * * * *".to_string(),
            working_dir: None,
            created_at: Utc::now(),
            enabled: true,
        }
    }

    fn make_run(job: &JobRecord, status: RunStatus) -> RunRecord {
        RunRecord {
            id: uuid::Uuid::new_v4().to_string(),
            job_id: job.id.clone(),
            job_name: job.name.clone().unwrap_or_else(|| job.id.clone()),
            started_at: Utc::now(),
            finished_at: None,
            exit_code: None,
            stdout: String::new(),
            stderr: String::new(),
            status,
        }
    }

    #[test]
    fn test_insert_and_list_jobs() {
        let store = Store::open_in_memory().unwrap();
        let job1 = make_job("aaaaaaaa", Some("backup"));
        let job2 = make_job("bbbbbbbb", Some("cleanup"));

        store.insert_job(&job1).unwrap();
        store.insert_job(&job2).unwrap();

        let jobs = store.list_jobs().unwrap();
        assert_eq!(jobs.len(), 2);
        // ordered by name
        assert_eq!(jobs[0].name, Some("backup".to_string()));
        assert_eq!(jobs[1].name, Some("cleanup".to_string()));
    }

    #[test]
    fn test_insert_job_without_name() {
        let store = Store::open_in_memory().unwrap();
        let job = make_job("aaaaaaaa", None);
        store.insert_job(&job).unwrap();

        let jobs = store.list_jobs().unwrap();
        assert_eq!(jobs.len(), 1);
        assert!(jobs[0].name.is_none());
    }

    #[test]
    fn test_duplicate_job_id() {
        let store = Store::open_in_memory().unwrap();
        let job = make_job("aaaaaaaa", Some("backup"));
        store.insert_job(&job).unwrap();

        let job2 = make_job("aaaaaaaa", Some("backup2"));
        let err = store.insert_job(&job2).unwrap_err();
        assert!(matches!(err, StoreError::JobAlreadyExists { .. }));
    }

    #[test]
    fn test_get_job_by_id_and_name() {
        let store = Store::open_in_memory().unwrap();
        let job = make_job("aaaaaaaa", Some("backup"));
        store.insert_job(&job).unwrap();

        // by id
        let found = store.get_job("aaaaaaaa").unwrap();
        assert_eq!(found.id, "aaaaaaaa");

        // by name
        let found = store.get_job("backup").unwrap();
        assert_eq!(found.id, "aaaaaaaa");
    }

    #[test]
    fn test_delete_job_by_id() {
        let store = Store::open_in_memory().unwrap();
        let job = make_job("aaaaaaaa", Some("backup"));
        store.insert_job(&job).unwrap();

        store.delete_job("aaaaaaaa").unwrap();
        let jobs = store.list_jobs().unwrap();
        assert_eq!(jobs.len(), 0);

        let err = store.delete_job("aaaaaaaa").unwrap_err();
        assert!(matches!(err, StoreError::JobNotFound { .. }));
    }

    #[test]
    fn test_insert_and_list_runs() {
        let store = Store::open_in_memory().unwrap();
        let job = make_job("aaaaaaaa", Some("backup"));
        store.insert_job(&job).unwrap();

        let run1 = make_run(&job, RunStatus::Success);
        let run2 = make_run(&job, RunStatus::Failed);
        store.insert_run(&run1).unwrap();
        store.insert_run(&run2).unwrap();

        let runs = store.list_runs("aaaaaaaa", 10).unwrap();
        assert_eq!(runs.len(), 2);
    }

    #[test]
    fn test_list_runs_backward_compat_by_name() {
        let store = Store::open_in_memory().unwrap();
        let job = make_job("aaaaaaaa", Some("backup"));
        store.insert_job(&job).unwrap();

        // Simulate old run stored with name as job_id
        let old_run = RunRecord {
            id: uuid::Uuid::new_v4().to_string(),
            job_id: "backup".to_string(),
            job_name: "backup".to_string(),
            started_at: Utc::now(),
            finished_at: None,
            exit_code: None,
            stdout: String::new(),
            stderr: String::new(),
            status: RunStatus::Success,
        };
        store.insert_run(&old_run).unwrap();

        // Query by name should find it via job_name column
        let runs = store.list_runs("backup", 10).unwrap();
        assert_eq!(runs.len(), 1);
    }

    #[test]
    fn test_get_last_run() {
        let store = Store::open_in_memory().unwrap();
        let job = make_job("aaaaaaaa", Some("backup"));
        store.insert_job(&job).unwrap();

        assert!(store.get_last_run("aaaaaaaa").unwrap().is_none());

        let run = make_run(&job, RunStatus::Success);
        store.insert_run(&run).unwrap();

        let last = store.get_last_run("aaaaaaaa").unwrap();
        assert!(last.is_some());
        assert_eq!(last.unwrap().status, RunStatus::Success);
    }

    #[test]
    fn test_update_run_not_found() {
        let store = Store::open_in_memory().unwrap();
        let job = make_job("aaaaaaaa", Some("backup"));
        store.insert_job(&job).unwrap();

        let mut run = make_run(&job, RunStatus::Running);
        run.id = "nonexistent-id".to_string();
        let err = store.update_run(&run).unwrap_err();
        assert!(matches!(err, StoreError::RunNotFound { .. }));
    }

    #[test]
    fn test_delete_job_also_has_runs() {
        let store = Store::open_in_memory().unwrap();
        let job = make_job("aaaaaaaa", Some("backup"));
        store.insert_job(&job).unwrap();

        let run = make_run(&job, RunStatus::Success);
        store.insert_run(&run).unwrap();

        store.delete_job("aaaaaaaa").unwrap();

        // Runs persist independently (no FK) — queried by job_id
        let runs = store.list_runs("aaaaaaaa", 10).unwrap();
        assert_eq!(runs.len(), 1);
    }

    #[test]
    #[cfg(unix)]
    fn test_store_sets_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("kron.db");
        let _store = Store::open(&db_path).unwrap();

        let meta = std::fs::metadata(&db_path).unwrap();
        assert_eq!(meta.permissions().mode() & 0o777, 0o600);
    }
}
