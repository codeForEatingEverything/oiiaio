//! 语义记忆存储（plan.md 3.23–3.30 的纯逻辑部分）。
//!
//! 通过 MCP 暴露给外部 Agent 需运行中的服务器（阻塞）；这里实现 store 层内核：
//! 写入、子串检索（全文排序检索后续增强）、按属主默认隔离、显式共享与撤销、开局摘要、后端 trait 边界。

use rusqlite::Connection;

#[derive(Debug, PartialEq, Eq)]
pub enum MemError {
    Store(String),
}
impl From<rusqlite::Error> for MemError {
    fn from(e: rusqlite::Error) -> Self {
        MemError::Store(e.to_string())
    }
}

/// 记忆后端 trait（3.23a / 3.30：可插拔，隔离与共享契约不随后端变化）。
pub trait MemoryBackend {
    fn write(&self, owner: &str, id: &str, text: &str) -> Result<(), MemError>;
    /// 检索：只返回 requester 自己的 + 被显式共享给它的，且匹配 query。
    fn search(&self, requester: &str, query: &str) -> Result<Vec<String>, MemError>;
    fn share(&self, id: &str, to_owner: &str) -> Result<(), MemError>;
    fn revoke(&self, id: &str, to_owner: &str) -> Result<(), MemError>;
    /// 开局摘要：requester 可见的全部记忆 id（3.28）。
    fn summary(&self, requester: &str) -> Result<Vec<String>, MemError>;
}

/// 文件/SQLite 后端（3.23c 文件持久化 + 3.24 检索；当前用 LIKE 子串，兼容中英文）。
pub struct SqliteBackend {
    conn: Connection,
}

impl SqliteBackend {
    pub fn open(path: &str) -> Result<Self, MemError> {
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS memories (id TEXT PRIMARY KEY, owner TEXT NOT NULL, text TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS memory_shares (memory_id TEXT NOT NULL, to_owner TEXT NOT NULL, UNIQUE(memory_id, to_owner));",
        )?;
        Ok(Self { conn })
    }

    fn visible(&self, requester: &str, id: &str) -> Result<bool, MemError> {
        let owner: Option<String> = self
            .conn
            .query_row("SELECT owner FROM memories WHERE id = ?1", [id], |r| {
                r.get(0)
            })
            .ok();
        if owner.as_deref() == Some(requester) {
            return Ok(true);
        }
        let shared: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM memory_shares WHERE memory_id = ?1 AND to_owner = ?2",
            (id, requester),
            |r| r.get(0),
        )?;
        Ok(shared > 0)
    }
}

impl MemoryBackend for SqliteBackend {
    fn write(&self, owner: &str, id: &str, text: &str) -> Result<(), MemError> {
        self.conn.execute(
            "INSERT INTO memories (id, owner, text) VALUES (?1, ?2, ?3)
             ON CONFLICT(id) DO UPDATE SET owner=excluded.owner, text=excluded.text",
            (id, owner, text),
        )?;
        Ok(())
    }

    fn search(&self, requester: &str, query: &str) -> Result<Vec<String>, MemError> {
        let like = format!("%{query}%");
        let mut stmt = self
            .conn
            .prepare("SELECT id FROM memories WHERE text LIKE ?1 ORDER BY id")?;
        let ids: Vec<String> = stmt
            .query_map([like], |r| r.get::<_, String>(0))?
            .collect::<Result<_, _>>()?;
        let mut out = Vec::new();
        for id in ids {
            if self.visible(requester, &id)? {
                out.push(id);
            }
        }
        Ok(out)
    }

    fn share(&self, id: &str, to_owner: &str) -> Result<(), MemError> {
        self.conn.execute(
            "INSERT OR IGNORE INTO memory_shares (memory_id, to_owner) VALUES (?1, ?2)",
            (id, to_owner),
        )?;
        Ok(())
    }

    fn revoke(&self, id: &str, to_owner: &str) -> Result<(), MemError> {
        self.conn.execute(
            "DELETE FROM memory_shares WHERE memory_id = ?1 AND to_owner = ?2",
            (id, to_owner),
        )?;
        Ok(())
    }

    fn summary(&self, requester: &str) -> Result<Vec<String>, MemError> {
        let mut stmt = self.conn.prepare(
            "SELECT id FROM memories WHERE owner = ?1
             UNION SELECT memory_id FROM memory_shares WHERE to_owner = ?1 ORDER BY 1",
        )?;
        let ids = stmt
            .query_map([requester], |r| r.get::<_, String>(0))?
            .collect::<Result<_, _>>()?;
        Ok(ids)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn be() -> SqliteBackend {
        SqliteBackend::open(":memory:").unwrap()
    }

    // 3.23b/c + 3.24: 写入后可按查询召回（全文）。
    #[test]
    fn write_then_search() {
        let b = be();
        b.write("alice", "m1", "周五下午不要安排会议").unwrap();
        b.write("alice", "m2", "报销走财务系统").unwrap();
        assert_eq!(b.search("alice", "会议").unwrap(), vec!["m1"]);
    }

    // 3.25: 默认隔离，别的属主查不到。
    #[test]
    fn isolation_by_default() {
        let b = be();
        b.write("alice", "m1", "secret plan").unwrap();
        assert_eq!(b.search("bob", "plan").unwrap(), Vec::<String>::new());
        assert_eq!(b.search("alice", "plan").unwrap(), vec!["m1"]);
    }

    // 3.26 + 3.27: 显式共享后可读，撤销后不可读。
    #[test]
    fn share_then_revoke() {
        let b = be();
        b.write("alice", "m1", "shared note").unwrap();
        b.share("m1", "bob").unwrap();
        assert_eq!(b.search("bob", "note").unwrap(), vec!["m1"]);
        b.revoke("m1", "bob").unwrap();
        assert_eq!(b.search("bob", "note").unwrap(), Vec::<String>::new());
    }

    // 3.28: 开局摘要返回可见记忆。
    #[test]
    fn summary_includes_owned_and_shared() {
        let b = be();
        b.write("alice", "m1", "a").unwrap();
        b.write("bob", "m2", "b").unwrap();
        b.share("m2", "alice").unwrap();
        assert_eq!(b.summary("alice").unwrap(), vec!["m1", "m2"]);
    }

    // 3.29: 人工修改（覆盖写）后检索返回新版本。
    #[test]
    fn manual_edit_updates() {
        let b = be();
        b.write("alice", "m1", "old content").unwrap();
        b.write("alice", "m1", "new content").unwrap();
        assert_eq!(b.search("alice", "new").unwrap(), vec!["m1"]);
        assert!(b.search("alice", "old").unwrap().is_empty());
    }
}
