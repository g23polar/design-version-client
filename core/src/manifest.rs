use rusqlite::{params, Connection};

use crate::error::{DvcError, Result};

// -- Schema -------------------------------------------------------------------

const SCHEMA_V1: &str = "
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

// -- Migrations ---------------------------------------------------------------

fn get_user_version(conn: &Connection) -> Result<i32> {
    let v: i32 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    Ok(v)
}

fn set_user_version(conn: &Connection, v: i32) -> Result<()> {
    conn.execute_batch(&format!("PRAGMA user_version = {v}"))?;
    Ok(())
}

fn run_migrations(conn: &Connection) -> Result<()> {
    let version = get_user_version(conn)?;

    if version < 1 {
        conn.execute_batch(SCHEMA_V1)?;
        set_user_version(conn, 1)?;
    }

    if version < 2 {
        // Milestone 2: add batch_id for multi-file directory snapshots.
        // Nullable for backward compat with existing single-file snapshots.
        let has_batch_id: bool = conn
            .prepare("SELECT 1 FROM pragma_table_info('snapshots') WHERE name = 'batch_id'")
            .and_then(|mut s| s.exists([]))
            .unwrap_or(false);
        if !has_batch_id {
            conn.execute_batch("ALTER TABLE snapshots ADD COLUMN batch_id TEXT")?;
        }
        conn.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_snapshots_batch ON snapshots(batch_id)",
        )?;
        set_user_version(conn, 2)?;
    }

    Ok(())
}

// -- Public structs -----------------------------------------------------------

#[derive(Debug, Clone, serde::Serialize)]
pub struct Project {
    pub id: i64,
    pub root_path: String,
    pub created_at: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Snapshot {
    pub id: i64,
    pub project_id: i64,
    pub file_path: String,
    pub blob_hash: String,
    pub file_size: u64,
    pub label: Option<String>,
    pub batch_id: Option<String>,
    pub created_at: String,
}

// -- Connection helpers -------------------------------------------------------

/// Open (or create) the manifest database and run migrations.
pub fn open(db_path: &std::path::Path) -> Result<Connection> {
    let conn = Connection::open(db_path)?;
    run_migrations(&conn)?;
    Ok(conn)
}

// -- Projects -----------------------------------------------------------------

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
    let mut stmt =
        conn.prepare("SELECT id, root_path, created_at FROM projects WHERE root_path = ?1")?;
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
    let mut stmt = conn.prepare("SELECT id, root_path, created_at FROM projects WHERE id = ?1")?;
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

// -- Snapshots ----------------------------------------------------------------

/// Insert a new snapshot record.  Returns the new row id.
pub fn insert_snapshot(
    conn: &Connection,
    project_id: i64,
    file_path: &str,
    blob_hash: &str,
    file_size: u64,
    label: Option<&str>,
) -> Result<i64> {
    insert_snapshot_with_batch(
        conn, project_id, file_path, blob_hash, file_size, label, None,
    )
}

/// Insert a snapshot with an optional batch_id.
pub fn insert_snapshot_with_batch(
    conn: &Connection,
    project_id: i64,
    file_path: &str,
    blob_hash: &str,
    file_size: u64,
    label: Option<&str>,
    batch_id: Option<&str>,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO snapshots (project_id, file_path, blob_hash, file_size, label, batch_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            project_id,
            file_path,
            blob_hash,
            file_size as i64,
            label,
            batch_id
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

fn row_to_snapshot(row: &rusqlite::Row<'_>) -> rusqlite::Result<Snapshot> {
    Ok(Snapshot {
        id: row.get(0)?,
        project_id: row.get(1)?,
        file_path: row.get(2)?,
        blob_hash: row.get(3)?,
        file_size: row.get::<_, i64>(4)? as u64,
        label: row.get(5)?,
        batch_id: row.get(6)?,
        created_at: row.get(7)?,
    })
}

const SNAPSHOT_COLS: &str =
    "id, project_id, file_path, blob_hash, file_size, label, batch_id, created_at";

/// List all snapshots for a project, ordered oldest-first.
pub fn list_snapshots(conn: &Connection, project_id: i64) -> Result<Vec<Snapshot>> {
    let sql = format!(
        "SELECT {SNAPSHOT_COLS} FROM snapshots WHERE project_id = ?1 ORDER BY created_at ASC"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![project_id], row_to_snapshot)?;
    let mut snapshots = Vec::new();
    for r in rows {
        snapshots.push(r?);
    }
    Ok(snapshots)
}

/// List snapshots filtered by label substring (case-insensitive).
pub fn list_snapshots_by_label(
    conn: &Connection,
    project_id: i64,
    pattern: &str,
) -> Result<Vec<Snapshot>> {
    let sql = format!(
        "SELECT {SNAPSHOT_COLS} FROM snapshots
         WHERE project_id = ?1 AND label LIKE '%' || ?2 || '%'
         ORDER BY created_at ASC"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![project_id, pattern], row_to_snapshot)?;
    let mut snapshots = Vec::new();
    for r in rows {
        snapshots.push(r?);
    }
    Ok(snapshots)
}

/// List snapshots filtered by file_path substring (case-insensitive).
pub fn list_snapshots_by_file(
    conn: &Connection,
    project_id: i64,
    pattern: &str,
) -> Result<Vec<Snapshot>> {
    let sql = format!(
        "SELECT {SNAPSHOT_COLS} FROM snapshots
         WHERE project_id = ?1 AND file_path LIKE '%' || ?2 || '%'
         ORDER BY created_at ASC"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![project_id, pattern], row_to_snapshot)?;
    let mut snapshots = Vec::new();
    for r in rows {
        snapshots.push(r?);
    }
    Ok(snapshots)
}

/// List snapshots by batch_id.
pub fn list_snapshots_by_batch(conn: &Connection, batch_id: &str) -> Result<Vec<Snapshot>> {
    let sql = format!(
        "SELECT {SNAPSHOT_COLS} FROM snapshots WHERE batch_id = ?1 ORDER BY created_at ASC"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![batch_id], row_to_snapshot)?;
    let mut snapshots = Vec::new();
    for r in rows {
        snapshots.push(r?);
    }
    Ok(snapshots)
}

/// Get a single snapshot by id.
pub fn get_snapshot(conn: &Connection, snapshot_id: i64) -> Result<Snapshot> {
    let sql = format!("SELECT {SNAPSHOT_COLS} FROM snapshots WHERE id = ?1");
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query(params![snapshot_id])?;
    match rows.next()? {
        Some(row) => Ok(row_to_snapshot(row)?),
        None => Err(DvcError::NotFound(format!("snapshot id={snapshot_id}"))),
    }
}

/// Delete a snapshot record (does NOT touch blobs).
pub fn delete_snapshot(conn: &Connection, snapshot_id: i64) -> Result<()> {
    conn.execute("DELETE FROM snapshots WHERE id = ?1", params![snapshot_id])?;
    Ok(())
}

/// Delete all snapshots for a batch (does NOT touch blobs).
pub fn delete_snapshots_by_batch(conn: &Connection, batch_id: &str) -> Result<u64> {
    let changed = conn.execute(
        "DELETE FROM snapshots WHERE batch_id = ?1",
        params![batch_id],
    )?;
    Ok(changed as u64)
}

/// Update the label on a single snapshot.
pub fn update_label(conn: &Connection, snapshot_id: i64, new_label: &str) -> Result<()> {
    let changed = conn.execute(
        "UPDATE snapshots SET label = ?2 WHERE id = ?1",
        params![snapshot_id, new_label],
    )?;
    if changed == 0 {
        return Err(DvcError::NotFound(format!("snapshot id={snapshot_id}")));
    }
    Ok(())
}

/// Update the label on all snapshots in a batch.
pub fn update_label_by_batch(conn: &Connection, batch_id: &str, new_label: &str) -> Result<u64> {
    let changed = conn.execute(
        "UPDATE snapshots SET label = ?2 WHERE batch_id = ?1",
        params![batch_id, new_label],
    )?;
    Ok(changed as u64)
}

/// Return every blob_hash that is referenced by at least one snapshot for a project.
pub fn referenced_hashes(conn: &Connection, project_id: i64) -> Result<Vec<String>> {
    let mut stmt =
        conn.prepare("SELECT DISTINCT blob_hash FROM snapshots WHERE project_id = ?1")?;
    let rows = stmt.query_map(params![project_id], |row| row.get(0))?;
    let mut hashes = Vec::new();
    for r in rows {
        hashes.push(r?);
    }
    Ok(hashes)
}

// -- Total storage ------------------------------------------------------------

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
        run_migrations(&conn).unwrap();
        conn
    }

    #[test]
    fn project_insert_and_find() {
        let conn = mem_db();
        let id = insert_project(&conn, "/tmp/myproject").unwrap();
        assert!(id > 0);
        let p = find_project_by_path(&conn, "/tmp/myproject")
            .unwrap()
            .unwrap();
        assert_eq!(p.id, id);
        assert_eq!(p.root_path, "/tmp/myproject");
    }

    #[test]
    fn find_missing_project_returns_none() {
        let conn = mem_db();
        assert!(find_project_by_path(&conn, "/nonexistent")
            .unwrap()
            .is_none());
    }

    #[test]
    fn snapshot_round_trip() {
        let conn = mem_db();
        let pid = insert_project(&conn, "/tmp/proj2").unwrap();
        let sid =
            insert_snapshot(&conn, pid, "design.psd", &"a".repeat(64), 1024, Some("v1")).unwrap();
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
        let sizes: Vec<u64> = snaps.iter().map(|s| s.file_size).collect();
        let mut sorted = sizes.clone();
        sorted.sort();
        assert_eq!(sizes, sorted);
    }

    #[test]
    fn migration_adds_batch_id() {
        let conn = mem_db();
        let pid = insert_project(&conn, "/tmp/batch_test").unwrap();
        let sid = insert_snapshot_with_batch(
            &conn,
            pid,
            "a.psd",
            &"c".repeat(64),
            512,
            Some("lbl"),
            Some("batch-001"),
        )
        .unwrap();
        let snap = get_snapshot(&conn, sid).unwrap();
        assert_eq!(snap.batch_id.as_deref(), Some("batch-001"));
    }

    #[test]
    fn list_by_label_filter() {
        let conn = mem_db();
        let pid = insert_project(&conn, "/tmp/label_test").unwrap();
        insert_snapshot(
            &conn,
            pid,
            "a.psd",
            &"d".repeat(64),
            100,
            Some("before-review"),
        )
        .unwrap();
        insert_snapshot(
            &conn,
            pid,
            "a.psd",
            &"e".repeat(64),
            200,
            Some("after-review"),
        )
        .unwrap();
        insert_snapshot(&conn, pid, "a.psd", &"f".repeat(64), 300, None).unwrap();

        let results = list_snapshots_by_label(&conn, pid, "review").unwrap();
        assert_eq!(results.len(), 2);

        let results = list_snapshots_by_label(&conn, pid, "before").unwrap();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn list_by_file_filter() {
        let conn = mem_db();
        let pid = insert_project(&conn, "/tmp/file_test").unwrap();
        insert_snapshot(&conn, pid, "logo.psd", &"d".repeat(64), 100, None).unwrap();
        insert_snapshot(&conn, pid, "banner.png", &"e".repeat(64), 200, None).unwrap();

        let results = list_snapshots_by_file(&conn, pid, "logo").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].file_path, "logo.psd");
    }

    #[test]
    fn update_label_works() {
        let conn = mem_db();
        let pid = insert_project(&conn, "/tmp/update_lbl").unwrap();
        let sid = insert_snapshot(&conn, pid, "a.psd", &"d".repeat(64), 100, Some("old")).unwrap();
        update_label(&conn, sid, "new").unwrap();
        let snap = get_snapshot(&conn, sid).unwrap();
        assert_eq!(snap.label.as_deref(), Some("new"));
    }

    #[test]
    fn list_and_delete_by_batch() {
        let conn = mem_db();
        let pid = insert_project(&conn, "/tmp/batch_ops").unwrap();
        let bid = "batch-xyz";
        insert_snapshot_with_batch(&conn, pid, "a.psd", &"a".repeat(64), 100, None, Some(bid))
            .unwrap();
        insert_snapshot_with_batch(&conn, pid, "b.png", &"b".repeat(64), 200, None, Some(bid))
            .unwrap();
        insert_snapshot(&conn, pid, "c.jpg", &"c".repeat(64), 300, None).unwrap();

        let batch = list_snapshots_by_batch(&conn, bid).unwrap();
        assert_eq!(batch.len(), 2);

        let deleted = delete_snapshots_by_batch(&conn, bid).unwrap();
        assert_eq!(deleted, 2);

        let all = list_snapshots(&conn, pid).unwrap();
        assert_eq!(all.len(), 1);
    }
}
