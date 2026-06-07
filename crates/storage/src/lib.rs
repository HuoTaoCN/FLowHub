use anyhow::Context;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileMetadata {
    pub id: String,
    pub path: String,
    pub filename: String,
    pub size_bytes: u64,
    pub sha256: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskMetadata {
    pub id: String,
    pub kind: String,
    pub status: String,
    pub target: String,
    pub progress: u64,
}

pub struct Storage {
    conn: Mutex<Connection>,
}

impl Storage {
    pub fn open(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let conn = Connection::open(path)?;
        let storage = Self {
            conn: Mutex::new(conn),
        };
        storage.migrate()?;
        Ok(storage)
    }

    pub fn in_memory() -> anyhow::Result<Self> {
        let conn = Connection::open_in_memory()?;
        let storage = Self {
            conn: Mutex::new(conn),
        };
        storage.migrate()?;
        Ok(storage)
    }

    pub fn store_file_metadata(&self, metadata: &FileMetadata) -> anyhow::Result<()> {
        let conn = self.conn.lock().expect("storage connection lock poisoned");
        conn.execute(
            "INSERT OR REPLACE INTO files (id, path, filename, size_bytes, sha256) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                metadata.id,
                metadata.path,
                metadata.filename,
                metadata.size_bytes as i64,
                metadata.sha256
            ],
        )?;
        Ok(())
    }

    pub fn get_file_metadata(&self, id: &str) -> anyhow::Result<Option<FileMetadata>> {
        let conn = self.conn.lock().expect("storage connection lock poisoned");
        let mut stmt =
            conn.prepare("SELECT id, path, filename, size_bytes, sha256 FROM files WHERE id = ?1")?;
        let mut rows = stmt.query(params![id])?;

        if let Some(row) = rows.next()? {
            Ok(Some(FileMetadata {
                id: row.get(0)?,
                path: row.get(1)?,
                filename: row.get(2)?,
                size_bytes: row.get::<_, i64>(3)? as u64,
                sha256: row.get(4)?,
            }))
        } else {
            Ok(None)
        }
    }

    pub fn upsert_task(&self, task: &TaskMetadata) -> anyhow::Result<()> {
        let conn = self.conn.lock().expect("storage connection lock poisoned");
        conn.execute(
            "INSERT OR REPLACE INTO tasks (id, kind, status, target, progress) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![task.id, task.kind, task.status, task.target, task.progress as i64],
        )?;
        Ok(())
    }

    pub fn update_task_status(&self, id: &str, status: &str) -> anyhow::Result<()> {
        let conn = self.conn.lock().expect("storage connection lock poisoned");
        conn.execute(
            "UPDATE tasks SET status = ?1 WHERE id = ?2",
            params![status, id],
        )
        .with_context(|| format!("failed to update task {id}"))?;
        Ok(())
    }

    pub fn update_task_progress(
        &self,
        id: &str,
        status: &str,
        progress: u64,
    ) -> anyhow::Result<()> {
        let conn = self.conn.lock().expect("storage connection lock poisoned");
        conn.execute(
            "UPDATE tasks SET status = ?1, progress = ?2 WHERE id = ?3",
            params![status, progress as i64, id],
        )
        .with_context(|| format!("failed to update task progress for {id}"))?;
        Ok(())
    }

    pub fn remove_task(&self, id: &str) -> anyhow::Result<()> {
        let conn = self.conn.lock().expect("storage connection lock poisoned");
        conn.execute("DELETE FROM tasks WHERE id = ?1", params![id])
            .with_context(|| format!("failed to remove task {id}"))?;
        Ok(())
    }

    pub fn list_tasks(&self) -> anyhow::Result<Vec<TaskMetadata>> {
        let conn = self.conn.lock().expect("storage connection lock poisoned");
        let mut stmt = conn
            .prepare("SELECT id, kind, status, target, progress FROM tasks ORDER BY rowid DESC")?;
        let rows = stmt.query_map([], |row| {
            Ok(TaskMetadata {
                id: row.get(0)?,
                kind: row.get(1)?,
                status: row.get(2)?,
                target: row.get(3)?,
                progress: row.get::<_, i64>(4)? as u64,
            })
        })?;

        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    fn migrate(&self) -> anyhow::Result<()> {
        let conn = self.conn.lock().expect("storage connection lock poisoned");
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS files (
                id TEXT PRIMARY KEY,
                path TEXT NOT NULL,
                filename TEXT NOT NULL,
                size_bytes INTEGER NOT NULL,
                sha256 TEXT
            );

            CREATE TABLE IF NOT EXISTS tasks (
                id TEXT PRIMARY KEY,
                kind TEXT NOT NULL,
                status TEXT NOT NULL,
                target TEXT NOT NULL,
                progress INTEGER NOT NULL DEFAULT 0
            );
            ",
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stores_and_reads_file_metadata() {
        let storage = Storage::in_memory().unwrap();
        let metadata = FileMetadata {
            id: "file-1".into(),
            path: "/tmp/a".into(),
            filename: "a".into(),
            size_bytes: 42,
            sha256: Some("abc".into()),
        };

        storage.store_file_metadata(&metadata).unwrap();
        assert_eq!(storage.get_file_metadata("file-1").unwrap(), Some(metadata));
    }

    #[test]
    fn updates_task_status() {
        let storage = Storage::in_memory().unwrap();
        storage
            .upsert_task(&TaskMetadata {
                id: "task-1".into(),
                kind: "download".into(),
                status: "queued".into(),
                target: "https://example.com/a".into(),
                progress: 0,
            })
            .unwrap();
        storage.update_task_status("task-1", "paused").unwrap();
        assert_eq!(storage.list_tasks().unwrap()[0].status, "paused");
    }

    #[test]
    fn updates_task_progress_and_removes_task() {
        let storage = Storage::in_memory().unwrap();
        storage
            .upsert_task(&TaskMetadata {
                id: "task-1".into(),
                kind: "download".into(),
                status: "queued".into(),
                target: "https://example.com/a".into(),
                progress: 0,
            })
            .unwrap();

        storage
            .update_task_progress("task-1", "active", 55)
            .unwrap();
        let task = storage.list_tasks().unwrap().remove(0);
        assert_eq!(task.status, "active");
        assert_eq!(task.progress, 55);

        storage.remove_task("task-1").unwrap();
        assert!(storage.list_tasks().unwrap().is_empty());
    }
}
