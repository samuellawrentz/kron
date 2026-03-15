use std::path::Path;

use rusqlite::{Connection, Row, params};
use rusqlite_migration::{M, Migrations};

use crate::error::StoreError;
use crate::models::{JobRecord, RunRecord, RunStatus};

const MIGRATIONS: &[M<'static>] = &[M::up(
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
        job_id      TEXT NOT NULL,
        job_name    TEXT NOT NULL,
        started_at  TEXT NOT NULL,
        finished_at TEXT,
        exit_code   INTEGER,
        stdout      TEXT NOT NULL DEFAULT '',
        stderr      TEXT NOT NULL DEFAULT '',
        status      TEXT NOT NULL DEFAULT 'running',
        FOREIGN KEY (job_id) REFERENCES jobs(id) ON DELETE CASCADE
    );
    CREATE INDEX idx_runs_job_started ON runs(job_name, started_at DESC);",
)];

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
        let conn = Connection::open(path)?;
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
                    name: job.name.clone(),
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

    pub fn get_job(&self, name: &str) -> Result<JobRecord, StoreError> {
        let result = self.conn.query_row(
            "SELECT id, name, command, schedule, working_dir, created_at, enabled
             FROM jobs WHERE name = ?1",
            params![name],
            job_from_row,
        );

        match result {
            Ok(job) => Ok(job),
            Err(rusqlite::Error::QueryReturnedNoRows) => Err(StoreError::JobNotFound {
                name: name.to_string(),
            }),
            Err(e) => Err(StoreError::Database(e)),
        }
    }

    pub fn delete_job(&self, name: &str) -> Result<(), StoreError> {
        let count = self
            .conn
            .execute("DELETE FROM jobs WHERE name = ?1", params![name])?;
        if count == 0 {
            return Err(StoreError::JobNotFound {
                name: name.to_string(),
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

    pub fn list_runs(&self, job_name: &str, limit: usize) -> Result<Vec<RunRecord>, StoreError> {
        let limit_i64 = i64::try_from(limit).unwrap_or(i64::MAX);
        let mut stmt = self.conn.prepare(
            "SELECT id, job_id, job_name, started_at, finished_at, exit_code, stdout, stderr, status
             FROM runs
             WHERE job_name = ?1
             ORDER BY started_at DESC
             LIMIT ?2",
        )?;

        let rows = stmt.query_map(params![job_name, limit_i64], run_from_row)?;

        let mut runs = Vec::new();
        for row in rows {
            runs.push(row?);
        }
        Ok(runs)
    }

    pub fn get_last_run(&self, job_name: &str) -> Result<Option<RunRecord>, StoreError> {
        let result = self.conn.query_row(
            "SELECT id, job_id, job_name, started_at, finished_at, exit_code, stdout, stderr, status
             FROM runs
             WHERE job_name = ?1
             ORDER BY started_at DESC
             LIMIT 1",
            params![job_name],
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

    fn make_job(name: &str) -> JobRecord {
        JobRecord {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.to_string(),
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
            job_name: job.name.clone(),
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
        let job1 = make_job("backup");
        let job2 = make_job("cleanup");

        store.insert_job(&job1).unwrap();
        store.insert_job(&job2).unwrap();

        let jobs = store.list_jobs().unwrap();
        assert_eq!(jobs.len(), 2);
        // ordered by name
        assert_eq!(jobs[0].name, "backup");
        assert_eq!(jobs[1].name, "cleanup");
    }

    #[test]
    fn test_duplicate_job_name() {
        let store = Store::open_in_memory().unwrap();
        let job = make_job("backup");
        store.insert_job(&job).unwrap();

        let job2 = make_job("backup");
        let err = store.insert_job(&job2).unwrap_err();
        assert!(matches!(err, StoreError::JobAlreadyExists { .. }));
    }

    #[test]
    fn test_delete_job() {
        let store = Store::open_in_memory().unwrap();
        let job = make_job("backup");
        store.insert_job(&job).unwrap();

        store.delete_job("backup").unwrap();
        let jobs = store.list_jobs().unwrap();
        assert_eq!(jobs.len(), 0);

        let err = store.delete_job("backup").unwrap_err();
        assert!(matches!(err, StoreError::JobNotFound { .. }));
    }

    #[test]
    fn test_insert_and_list_runs() {
        let store = Store::open_in_memory().unwrap();
        let job = make_job("backup");
        store.insert_job(&job).unwrap();

        let run1 = make_run(&job, RunStatus::Success);
        let run2 = make_run(&job, RunStatus::Failed);
        store.insert_run(&run1).unwrap();
        store.insert_run(&run2).unwrap();

        let runs = store.list_runs("backup", 10).unwrap();
        assert_eq!(runs.len(), 2);
    }

    #[test]
    fn test_get_last_run() {
        let store = Store::open_in_memory().unwrap();
        let job = make_job("backup");
        store.insert_job(&job).unwrap();

        assert!(store.get_last_run("backup").unwrap().is_none());

        let run = make_run(&job, RunStatus::Success);
        store.insert_run(&run).unwrap();

        let last = store.get_last_run("backup").unwrap();
        assert!(last.is_some());
        assert_eq!(last.unwrap().status, RunStatus::Success);
    }

    #[test]
    fn test_update_run_not_found() {
        let store = Store::open_in_memory().unwrap();
        let job = make_job("backup");
        store.insert_job(&job).unwrap();

        let mut run = make_run(&job, RunStatus::Running);
        run.id = "nonexistent-id".to_string();
        let err = store.update_run(&run).unwrap_err();
        assert!(matches!(err, StoreError::RunNotFound { .. }));
    }

    #[test]
    fn test_cascade_delete() {
        let store = Store::open_in_memory().unwrap();
        let job = make_job("backup");
        store.insert_job(&job).unwrap();

        let run = make_run(&job, RunStatus::Success);
        store.insert_run(&run).unwrap();

        store.delete_job("backup").unwrap();

        // runs should be gone due to cascade
        let runs = store.list_runs("backup", 10).unwrap();
        assert_eq!(runs.len(), 0);
    }
}
