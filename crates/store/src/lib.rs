//! 元数据存储（plan.md 0.08–0.10）。
//!
//! 本地用 SQLite（Straw.md §3）。先实现最小的键值元数据读写，
//! 之后按同一读写契约接入 Postgres（0.09）。

use rusqlite::Connection;

pub mod migrate;

/// 最小元数据存储句柄。
pub struct MetaStore {
    conn: Connection,
}

/// 存储错误。
#[derive(Debug)]
pub enum StoreError {
    Backend(String),
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StoreError::Backend(m) => write!(f, "存储后端错误: {m}"),
        }
    }
}
impl std::error::Error for StoreError {}

impl From<rusqlite::Error> for StoreError {
    fn from(e: rusqlite::Error) -> Self {
        StoreError::Backend(e.to_string())
    }
}

impl MetaStore {
    /// 打开指定路径的 SQLite 库（":memory:" 为内存库）。
    pub fn open(path: &str) -> Result<Self, StoreError> {
        let conn = Connection::open(path)?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS meta (k TEXT PRIMARY KEY, v TEXT NOT NULL)",
            [],
        )?;
        Ok(Self { conn })
    }

    /// 写入一个键值。
    pub fn put(&self, key: &str, value: &str) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT INTO meta (k, v) VALUES (?1, ?2)
             ON CONFLICT(k) DO UPDATE SET v = excluded.v",
            (key, value),
        )?;
        Ok(())
    }

    /// 读回一个键值。
    pub fn get(&self, key: &str) -> Result<Option<String>, StoreError> {
        let mut stmt = self.conn.prepare("SELECT v FROM meta WHERE k = ?1")?;
        let mut rows = stmt.query([key])?;
        match rows.next()? {
            Some(row) => Ok(Some(row.get(0)?)),
            None => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // 0.08: 本地元数据写入后可读回。
    #[test]
    fn put_then_get_roundtrip() {
        let s = MetaStore::open(":memory:").unwrap();
        s.put("node:a", "online").unwrap();
        assert_eq!(s.get("node:a").unwrap(), Some("online".to_string()));
        assert_eq!(s.get("node:missing").unwrap(), None);
    }

    // 0.10: 重启（重新打开同一文件库）后元数据仍存在。
    #[test]
    fn metadata_survives_reopen() {
        let mut path = std::env::temp_dir();
        path.push(format!("oiiaio-store-{}.db", std::process::id()));
        let p = path.to_str().unwrap();
        let _ = std::fs::remove_file(p);
        {
            let s = MetaStore::open(p).unwrap();
            s.put("k", "v1").unwrap();
        } // 连接在此关闭，模拟进程重启
        {
            let s = MetaStore::open(p).unwrap();
            assert_eq!(s.get("k").unwrap(), Some("v1".to_string()));
        }
        let _ = std::fs::remove_file(p);
    }
}
