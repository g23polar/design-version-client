use rusqlite::{Connection, params};

use crate::error::{DvcError, Result};

// ── Schema ────────────────────────────────────────────────────────────────────

const SCHEMA: &str = "
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS projects (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    root_path  TEXT    NOT NULL UNIQUE,
    created_at TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

CREATE TABLE IF NOT EXISTS snapshots (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id INTEGER NOT NULL REFERENCES projects(id),
    file_path  TEXT    NOT NULL,
    blob_hash  TEXT    NOT NULL,
    file_size  INTEGER NOT NULL,
    label      TEXT,
    created_at TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_snapshots_project ON snapshots(project_id, created_at);
";

// ── Public structs ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct Project {
    pub id: i64,
    pub root_path: String,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct Snapshot {
    pub id: i64,
    pub project_id: i64,
    pub file_path: String,
    pub blob_hash: String,
    pub file_size: u64,
    pub label: Option<String>,
    pub created_at: String,
}

// ── Connection helpers ─────────────────────────────────────────────────────────

/// Open (or create) the manifest database and run migrations.
pub fn open(db_path: &std::path::Path) -> Result<Connection> {
    let conn = Connection::open(db_path)?;
    conn.execute_batch(SCHEMA)?;
    Ok(conn)
}

// ── Projects ──────────────────────────────────────────────────────────────────

/// Insert a new project.  Returns the new row id.
pub fn insert_project(conn: &Connection, root_path: &str) -> Result<i64> {
    conn.execute(
        "INSERT INTO projects (root_path) VALUES (?1)",
        params![root_path],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Find a project by its root path.
pub fn find_project_by_path(conn: &Connection, root_path: &str) -> Result<Option<Project>> {
    let mut stmt = conn.prepare(
        "SELECT id, root_path, created_at FROM projects WHERE root_path = ?1",
    )?;
    let mut rows = stmt.query(params![root_path])?;
    match rows.next()? {
        Some(row) => Ok(Some(Project {
            id: row.get(0)?,
            root_path: row.get(1)?,
            created_at: row.get(2)?,
        })),
        None => Ok(None),
    }
}

/// Get a project by id.
pub fn get_project(conn: &Connection, project_id: i64) -> Result<Project> {
    let mut stmt = conn.prepare(
        "SELECT id, root_path, created_at FROM projects WHERE id = ?1",
    )?;
    let mut rows = stmt.query(params![project_id])?;
    match rows.next()? {
        Some(row) => Ok(Project {
            id: row.get(0)?,
            root_path: row.get(1)?,
            created_at: row.get(2)?,
        }),
        None => Err(DvcError::NotFound(format!("project id={project_id}"))),
    }
}

// ── Snapshots ─────────────────────────────────────────────────────────────────

/// Insert a new snapshot record.  Returns the new row id.
pub fn insert_snapshot(
    conn: &Connection,
    project_id: i64,
    file_path: &str,
    blob_hash: &str,
    file_size: u64,
    label: Option<&str>,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO snapshots (project_id, file_path, blob_hash, file_size, label)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![project_id, file_path, blob_hash, file_size as i64, label],
    )?;
    Ok(conn.last_insert_rowid())
}

/// List all snapshots for a project, ordered oldest-first.
pub fn list_snapshots(conn: &Connection, project_id: i64) -> Result<Vec<Snapshot>> {
    let mut stmt = conn.prepare(
        "SELECT id, project_id, file_path, blob_hash, file_size, label, created_at
         FROM snapshots
         WHERE project_id = ?1
         ORDER BY created_at ASC",
    )?;
    let rows = stmt.query_map(params![project_id], |row| {
        Ok(Snapshot {
            id: row.get(0)?,
            project_id: row.get(1)?,
            file_path: row.get(2)?,
            blob_hash: row.get(3)?,
            file_size: row.get::<_, i64>(4)? as u64,
            label: row.get(5)?,
            created_at: row.get(6)?,
        })
    })?;

    let mut snapshots = Vec::new();
    for r in rows {
        snapshots.push(r?);
    }
    Ok(snapshots)
}

/// Get a single snapshot by id.
pub fn get_snapshot(conn: &Connection, snapshot_id: i64) -> Result<Snapshot> {
    let mut stmt = conn.prepare(
        "SELECT id, project_id, file_path, blob_hash, file_size, label, created_at
         FROM snapshots WHERE id = ?1",
    )?;
    let mut rows = stmt.query(params![snapshot_id])?;
    match rows.next()? {
        Some(row) => Ok(Snapshot {
            id: row.get(0)?,
            project_id: row.get(1)?,
            file_path: row.get(2)?,
            blob_hash: row.get(3)?,
            file_size: row.get::<_, i64>(4)? as u64,
            label: row.get(5)?,
            created_at: row.get(6)?,
        }),
        None => Err(DvcError::NotFound(format!("snapshot id={snapshot_id}"))),
    }
}

/// Delete a snapshot record (does NOT touch blobs).
pub fn delete_snapshot(conn: &Connection, snapshot_id: i64) -> Result<()> {
    conn.execute("DELETE FROM snapshots WHERE id = ?1", params![snapshot_id])?;
    Ok(())
}

/// Return every blob_hash that is referenced by at least one snapshot for a project.
pub fn referenced_hashes(conn: &Connection, project_id: i64) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT blob_hash FROM snapshots WHERE project_id = ?1",
    )?;
    let rows = stmt.query_map(params![project_id], |row| row.get(0))?;
    let mut hashes = Vec::new();
    for r in rows {
        hashes.push(r?);
    }
    Ok(hashes)
}

// ── Total storage ──────────────────────────────────────────────────────────────

/// Sum of file_size for all snapshots under a project (logical, not deduplicated).
pub fn total_logical_bytes(conn: &Connection, project_id: i64) -> Result<u64> {
    let bytes: i64 = conn.query_row(
        "SELECT COALESCE(SUM(file_size), 0) FROM snapshots WHERE project_id = ?1",
        params![project_id],
        |row| row.get(0),
    )?;
    Ok(bytes as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA).unwrap();
        conn
    }

    #[test]
    fn project_insert_and_find() {
        let conn = mem_db();
        let id = insert_project(&conn, "/tmp/myproject").unwrap();
        assert!(id > 0);
        let p = find_project_by_path(&conn, "/tmp/myproject").unwrap().unwrap();
        assert_eq!(p.id, id);
        assert_eq!(p.root_path, "/tmp/myproject");
    }

    #[test]
    fn find_missing_project_returns_none() {
        let conn = mem_db();
        assert!(find_project_by_path(&conn, "/nonexistent").unwrap().is_none());
    }

    #[test]
    fn snapshot_round_trip() {
        let conn = mem_db();
        let pid = insert_project(&conn, "/tmp/proj2").unwrap();
        let sid = insert_snapshot(
            &conn,
            pid,
            "design.psd",
            &"a".repeat(64),
            1024,
            Some("v1"),
        )
        .unwrap();
        assert!(sid > 0);

        let snaps = list_snapshots(&conn, pid).unwrap();
        assert_eq!(snaps.len(), 1);
        assert_eq!(snaps[0].label.as_deref(), Some("v1"));
        assert_eq!(snaps[0].file_size, 1024);
    }

    #[test]
    fn multiple_snapshots_ordered() {
        let conn = mem_db();
        let pid = insert_project(&conn, "/tmp/proj3").unwrap();
        for i in 0..5u64 {
            insert_snapshot(&conn, pid, "f.psd", &"b".repeat(64), i * 100, None).unwrap();
        }
        let snaps = list_snapshots(&conn, pid).unwrap();
        assert_eq!(snaps.len(), 5);
        // file_size used as a proxy for insertion order in this test
        let sizes: Vec<u64> = snaps.iter().map(|s| s.file_size).collect();
        let mut sorted = sizes.clone();
        sorted.sort();
        assert_eq!(sizes, sorted);
    }
}
