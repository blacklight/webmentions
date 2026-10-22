//! SQLite storage backend — mirrors
//! `webmentions.storage.adapters.db`.
//!
//! The Python adapter is generic over SQLAlchemy engines; the Rust
//! port ships a rusqlite-backed implementation — SQLite is what the
//! vast majority of SQLAlchemy deployments use for this library, and
//! it keeps the crate dependency-free of an ORM.
//!
//! One deliberate deviation: the Python `DbWebmention` model declares
//! `published` as non-nullable, so mentions whose source carries no
//! date cannot be stored through the SQLAlchemy adapter (the insert
//! fails). Here `published` is nullable — the field is legitimately
//! absent on real-world mentions.
//!
//! Enums are stored as their ``.value`` strings (`"incoming"`,
//! `"confirmed"`, `"like"`, …) and datetimes as `isoformat()` strings.

use std::path::Path;
use std::sync::Mutex;

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};

use crate::error::{Result, WebmentionError};
use crate::model::{
    dt_to_iso, parse_dt_str, Webmention, WebmentionDirection, WebmentionStatus, WebmentionType,
};

use super::WebmentionsStorage;

const DEFAULT_TABLE: &str = "webmentions";

/// SQLite-backed Webmentions storage — the Rust equivalent of
/// `DbWebmentionsStorage` + `init_db_storage`.
///
/// Uses a single `rusqlite::Connection` behind a mutex — matching the
/// session-per-call, single-writer behavior of the Python adapter.
pub struct SqliteWebmentionsStorage {
    conn: Mutex<Connection>,
    table_name: String,
}

impl SqliteWebmentionsStorage {
    /// Initialize a SQLite storage at `path` (creating the table if
    /// needed) — mirrors `init_db_storage(engine="sqlite:///…")`.
    ///
    /// Pass `:memory:` for an in-memory database.
    pub fn new(path: impl AsRef<Path>) -> Result<Self> {
        Self::with_table_name(path, DEFAULT_TABLE)
    }

    /// Initialize a SQLite storage with a custom table name — mirrors
    /// `init_db_storage(…, table_name=…)`.
    pub fn with_table_name(path: impl AsRef<Path>, table_name: &str) -> Result<Self> {
        let conn = if path.as_ref().as_os_str() == ":memory:" {
            Connection::open_in_memory()
        } else {
            Connection::open(path)
        }
        .map_err(|e| WebmentionError::General(format!("failed to open database: {e}")))?;
        let storage = SqliteWebmentionsStorage {
            conn: Mutex::new(conn),
            table_name: table_name.to_string(),
        };
        storage.create_schema()?;
        Ok(storage)
    }

    fn create_schema(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch(&format!(
            "CREATE TABLE IF NOT EXISTS {table} (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                source TEXT NOT NULL,
                target TEXT NOT NULL,
                direction TEXT NOT NULL,
                title TEXT,
                excerpt TEXT,
                content TEXT,
                author_name TEXT,
                author_url TEXT,
                author_photo TEXT,
                published TEXT,
                status TEXT NOT NULL DEFAULT 'confirmed',
                mention_type TEXT NOT NULL,
                meta TEXT NOT NULL DEFAULT '{{}}',
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                UNIQUE (source, target, direction)
            )",
            table = self.table_name
        ))
        .map_err(|e| WebmentionError::General(format!("failed to create schema: {e}")))
    }

    fn row_to_webmention(row: &rusqlite::Row) -> rusqlite::Result<Webmention> {
        let get_s = |name: &str| row.get::<_, Option<String>>(name);
        let direction_raw: String = row.get("direction")?;
        let status_raw: Option<String> = row.get("status")?;
        let type_raw: Option<String> = row.get("mention_type")?;
        let meta_raw: Option<String> = row.get("meta")?;
        let parse_date =
            |v: Option<String>| -> Option<DateTime<Utc>> { v.and_then(|s| parse_dt_str(&s)) };
        Ok(Webmention {
            source: row.get("source")?,
            target: row.get("target")?,
            direction: WebmentionDirection::from_raw(&direction_raw)
                .unwrap_or(WebmentionDirection::In),
            title: get_s("title")?,
            excerpt: get_s("excerpt")?,
            content: get_s("content")?,
            author_name: get_s("author_name")?,
            author_url: get_s("author_url")?,
            author_photo: get_s("author_photo")?,
            published: parse_date(get_s("published")?),
            status: status_raw
                .and_then(|s| WebmentionStatus::from_raw(&s).ok())
                .unwrap_or(WebmentionStatus::Confirmed),
            mention_type: WebmentionType::from_raw(type_raw.as_deref()),
            mention_type_raw: None,
            metadata: meta_raw
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or(serde_json::json!({})),
            created_at: parse_date(get_s("created_at")?),
            updated_at: parse_date(get_s("updated_at")?),
        })
    }
}

fn db_err(e: rusqlite::Error) -> WebmentionError {
    WebmentionError::General(format!("database error: {e}"))
}

impl WebmentionsStorage for SqliteWebmentionsStorage {
    /// Store a Webmention — mirrors `DbWebmentionsStorage.store_webmention`.
    ///
    /// Insert-or-update on the `(source, target, direction)` unique key;
    /// `created_at` is preserved across updates while `updated_at` is
    /// bumped — the same semantics as the Python fallback path.
    fn store_webmention(&self, mention: &Webmention) -> Result<()> {
        let now = Utc::now();
        let created_at = mention.created_at.or(mention.published).unwrap_or(now);
        let conn = self.conn.lock().unwrap();
        conn.execute(
            &format!(
                "INSERT INTO {table} (
                    source, target, direction, title, excerpt, content,
                    author_name, author_url, author_photo, published,
                    status, mention_type, meta, created_at, updated_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
                ON CONFLICT (source, target, direction) DO UPDATE SET
                    title = excluded.title,
                    excerpt = excluded.excerpt,
                    content = excluded.content,
                    author_name = excluded.author_name,
                    author_url = excluded.author_url,
                    author_photo = excluded.author_photo,
                    published = excluded.published,
                    status = excluded.status,
                    mention_type = excluded.mention_type,
                    meta = excluded.meta,
                    updated_at = excluded.updated_at",
                table = self.table_name
            ),
            params![
                mention.source,
                mention.target,
                mention.direction.as_str(),
                mention.title,
                mention.excerpt,
                mention.content,
                mention.author_name,
                mention.author_url,
                mention.author_photo,
                mention.published.as_ref().map(dt_to_iso),
                mention.status.as_str(),
                mention.mention_type.as_str(),
                serde_json::to_string(&mention.metadata).unwrap_or_else(|_| "{}".into()),
                dt_to_iso(&created_at),
                dt_to_iso(&now),
            ],
        )
        .map_err(db_err)?;
        Ok(())
    }

    fn delete_webmention(
        &self,
        source: &str,
        target: &str,
        direction: WebmentionDirection,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            &format!(
                "DELETE FROM {table} WHERE source = ?1 AND target = ?2 AND direction = ?3",
                table = self.table_name
            ),
            params![source, target, direction.as_str()],
        )
        .map_err(db_err)?;
        Ok(())
    }

    fn retrieve_webmentions(
        &self,
        resource: &str,
        direction: WebmentionDirection,
    ) -> Result<Vec<Webmention>> {
        let resource_column = if direction == WebmentionDirection::Out {
            "source"
        } else {
            "target"
        };
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(&format!(
                "SELECT * FROM {table} WHERE {col} = ?1 AND direction = ?2 AND status = 'confirmed'
                 ORDER BY id",
                table = self.table_name,
                col = resource_column
            ))
            .map_err(db_err)?;
        let rows = stmt
            .query_map(
                params![resource, direction.as_str()],
                Self::row_to_webmention,
            )
            .map_err(db_err)?;
        let mut mentions = Vec::new();
        for row in rows {
            mentions.push(row.map_err(db_err)?);
        }
        Ok(mentions)
    }
}
