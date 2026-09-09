use std::sync::Mutex;

use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};

pub struct Db(Mutex<Connection>);

impl Db {
    pub fn open(path: &std::path::Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path).context("open sqlite")?;
        conn.execute_batch(
            "
            PRAGMA foreign_keys = ON;
            PRAGMA journal_mode = WAL;
            CREATE TABLE IF NOT EXISTS users (
                id INTEGER PRIMARY KEY,
                username TEXT NOT NULL UNIQUE,
                password_hash TEXT NOT NULL,
                created_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS sessions (
                token TEXT PRIMARY KEY,
                user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
                created_at TEXT NOT NULL,
                expires_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS apps (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                compose_yaml TEXT NOT NULL,
                icon_url TEXT,
                web_port INTEGER,
                created_at TEXT NOT NULL,
                last_error TEXT
            );
            CREATE TABLE IF NOT EXISTS kv (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );
            ",
        )?;
        let _ = conn.execute("ALTER TABLE apps ADD COLUMN last_error TEXT", []);
        Ok(Self(Mutex::new(conn)))
    }

    pub fn user_count(&self) -> Result<u32> {
        let conn = self.0.lock().unwrap();
        let n: u32 = conn.query_row("SELECT COUNT(*) FROM users", [], |r| r.get(0))?;
        Ok(n)
    }

    pub fn create_user(&self, username: &str, password_hash: &str) -> Result<i64> {
        let conn = self.0.lock().unwrap();
        let now = now_rfc3339();
        conn.execute(
            "INSERT INTO users (username, password_hash, created_at) VALUES (?1, ?2, ?3)",
            params![username, password_hash, now],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn user_by_name(&self, username: &str) -> Result<Option<UserRow>> {
        let conn = self.0.lock().unwrap();
        let row = conn
            .query_row(
                "SELECT id, username, password_hash FROM users WHERE username = ?1",
                params![username],
                |r| {
                    Ok(UserRow {
                        id: r.get(0)?,
                        username: r.get(1)?,
                        password_hash: r.get(2)?,
                    })
                },
            )
            .optional()?;
        Ok(row)
    }

    pub fn set_password_hash(&self, user_id: i64, password_hash: &str) -> Result<()> {
        let conn = self.0.lock().unwrap();
        conn.execute(
            "UPDATE users SET password_hash = ?1 WHERE id = ?2",
            params![password_hash, user_id],
        )?;
        Ok(())
    }

    pub fn insert_session(&self, token: &str, user_id: i64, expires_at: &str) -> Result<()> {
        let conn = self.0.lock().unwrap();
        conn.execute(
            "INSERT INTO sessions (token, user_id, created_at, expires_at) VALUES (?1, ?2, ?3, ?4)",
            params![token, user_id, now_rfc3339(), expires_at],
        )?;
        Ok(())
    }

    pub fn session_user(&self, token: &str) -> Result<Option<UserRow>> {
        let conn = self.0.lock().unwrap();
        let now = now_rfc3339();
        let row = conn
            .query_row(
                "SELECT u.id, u.username, u.password_hash
                 FROM sessions s JOIN users u ON u.id = s.user_id
                 WHERE s.token = ?1 AND s.expires_at > ?2",
                params![token, now],
                |r| {
                    Ok(UserRow {
                        id: r.get(0)?,
                        username: r.get(1)?,
                        password_hash: r.get(2)?,
                    })
                },
            )
            .optional()?;
        Ok(row)
    }

    pub fn delete_session(&self, token: &str) -> Result<()> {
        let conn = self.0.lock().unwrap();
        conn.execute("DELETE FROM sessions WHERE token = ?1", params![token])?;
        Ok(())
    }

    pub fn list_apps(&self) -> Result<Vec<AppRow>> {
        let conn = self.0.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, name, compose_yaml, icon_url, web_port, created_at, last_error FROM apps ORDER BY name",
        )?;
        let rows = stmt.query_map([], map_app)?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn get_app(&self, id: &str) -> Result<Option<AppRow>> {
        let conn = self.0.lock().unwrap();
        let row = conn
            .query_row(
                "SELECT id, name, compose_yaml, icon_url, web_port, created_at, last_error FROM apps WHERE id = ?1",
                params![id],
                map_app,
            )
            .optional()?;
        Ok(row)
    }

    pub fn upsert_app(&self, app: &AppRow) -> Result<()> {
        let conn = self.0.lock().unwrap();
        conn.execute(
            "INSERT INTO apps (id, name, compose_yaml, icon_url, web_port, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                compose_yaml = excluded.compose_yaml,
                icon_url = excluded.icon_url,
                web_port = excluded.web_port",
            params![
                app.id,
                app.name,
                app.compose_yaml,
                app.icon_url,
                app.web_port,
                app.created_at
            ],
        )?;
        Ok(())
    }

    pub fn delete_app(&self, id: &str) -> Result<()> {
        let conn = self.0.lock().unwrap();
        conn.execute("DELETE FROM apps WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn set_last_error(&self, id: &str, err: Option<&str>) -> Result<()> {
        let conn = self.0.lock().unwrap();
        conn.execute(
            "UPDATE apps SET last_error = ?1 WHERE id = ?2",
            params![err, id],
        )?;
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct UserRow {
    pub id: i64,
    pub username: String,
    pub password_hash: String,
}

#[derive(Debug, Clone)]
pub struct AppRow {
    pub id: String,
    pub name: String,
    pub compose_yaml: String,
    pub icon_url: Option<String>,
    pub web_port: Option<i64>,
    pub created_at: String,
    pub last_error: Option<String>,
}

fn map_app(r: &rusqlite::Row) -> rusqlite::Result<AppRow> {
    Ok(AppRow {
        id: r.get(0)?,
        name: r.get(1)?,
        compose_yaml: r.get(2)?,
        icon_url: r.get(3)?,
        web_port: r.get(4)?,
        created_at: r.get(5)?,
        last_error: r.get(6)?,
    })
}

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339()
}
