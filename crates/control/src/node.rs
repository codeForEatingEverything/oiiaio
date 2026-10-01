//! 节点注册、版本协商、心跳与失联（plan.md 1.01–1.04）。

use rusqlite::Connection;

use oiiaio_store::StoreError;

/// 控制面支持的节点协议大版本集合。
pub const SUPPORTED_PROTOCOL_MAJORS: &[u32] = &[1];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    pub id: String,
    pub protocol_major: u32,
    pub agent_version: String,
    pub last_seen_ms: i64,
}

#[derive(Debug, PartialEq, Eq)]
pub enum NodeError {
    Store(String),
    /// 协议大版本不兼容（1.01b）。
    IncompatibleProtocol {
        got: u32,
    },
}

impl From<StoreError> for NodeError {
    fn from(e: StoreError) -> Self {
        NodeError::Store(e.to_string())
    }
}
impl From<rusqlite::Error> for NodeError {
    fn from(e: rusqlite::Error) -> Self {
        NodeError::Store(e.to_string())
    }
}

pub fn init_schema(conn: &Connection) -> Result<(), NodeError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS nodes (
            id TEXT PRIMARY KEY,
            protocol_major INTEGER NOT NULL,
            agent_version TEXT NOT NULL,
            last_seen_ms INTEGER NOT NULL
        )",
    )?;
    Ok(())
}

fn is_compatible(major: u32) -> bool {
    SUPPORTED_PROTOCOL_MAJORS.contains(&major)
}

/// 注册节点（1.01 记录版本；1.01a 协商；1.01b 拒绝不兼容；1.02 幂等）。
pub fn register(
    conn: &Connection,
    id: &str,
    protocol_major: u32,
    agent_version: &str,
    now_ms: i64,
) -> Result<Node, NodeError> {
    if !is_compatible(protocol_major) {
        return Err(NodeError::IncompatibleProtocol {
            got: protocol_major,
        });
    }
    // UPSERT：重复注册更新而非新增（1.02 幂等）。
    conn.execute(
        "INSERT INTO nodes (id, protocol_major, agent_version, last_seen_ms)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(id) DO UPDATE SET
            protocol_major = excluded.protocol_major,
            agent_version  = excluded.agent_version,
            last_seen_ms   = excluded.last_seen_ms",
        (id, protocol_major, agent_version, now_ms),
    )?;
    Ok(Node {
        id: id.to_string(),
        protocol_major,
        agent_version: agent_version.to_string(),
        last_seen_ms: now_ms,
    })
}

pub fn get(conn: &Connection, id: &str) -> Result<Option<Node>, NodeError> {
    let mut stmt = conn.prepare(
        "SELECT id, protocol_major, agent_version, last_seen_ms FROM nodes WHERE id = ?1",
    )?;
    let mut rows = stmt.query([id])?;
    match rows.next()? {
        Some(r) => Ok(Some(Node {
            id: r.get(0)?,
            protocol_major: r.get::<_, i64>(1)? as u32,
            agent_version: r.get(2)?,
            last_seen_ms: r.get(3)?,
        })),
        None => Ok(None),
    }
}

pub fn count(conn: &Connection) -> Result<i64, NodeError> {
    Ok(conn.query_row("SELECT COUNT(*) FROM nodes", [], |r| r.get(0))?)
}

/// 心跳：更新在线时间（1.03）。
pub fn heartbeat(conn: &Connection, id: &str, now_ms: i64) -> Result<(), NodeError> {
    let n = conn.execute(
        "UPDATE nodes SET last_seen_ms = ?1 WHERE id = ?2",
        (now_ms, id),
    )?;
    if n == 0 {
        return Err(NodeError::Store(format!("节点不存在: {id}")));
    }
    Ok(())
}

/// 是否接受新分配：心跳未超时才接受（1.04）。
pub fn is_accepting(node: &Node, now_ms: i64, timeout_ms: i64) -> bool {
    now_ms - node.last_seen_ms <= timeout_ms
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conn() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        init_schema(&c).unwrap();
        c
    }

    // 1.01 + 1.01a: 注册后可查询版本与能力，兼容版本协商成功。
    #[test]
    fn register_records_version() {
        let c = conn();
        let n = register(&c, "node-a", 1, "0.1.0", 1000).unwrap();
        assert_eq!(n.protocol_major, 1);
        assert_eq!(get(&c, "node-a").unwrap().unwrap().agent_version, "0.1.0");
    }

    // 1.01b: 不兼容版本被明确拒绝。
    #[test]
    fn incompatible_protocol_rejected() {
        let c = conn();
        let err = register(&c, "node-x", 999, "9.9.9", 1000).unwrap_err();
        assert_eq!(err, NodeError::IncompatibleProtocol { got: 999 });
        assert!(get(&c, "node-x").unwrap().is_none());
    }

    // 1.02: 重复注册不产生两个节点。
    #[test]
    fn register_is_idempotent() {
        let c = conn();
        register(&c, "node-a", 1, "0.1.0", 1000).unwrap();
        register(&c, "node-a", 1, "0.2.0", 2000).unwrap();
        assert_eq!(count(&c).unwrap(), 1);
        assert_eq!(get(&c, "node-a").unwrap().unwrap().agent_version, "0.2.0");
    }

    // 1.03: 心跳更新在线时间。
    #[test]
    fn heartbeat_updates_last_seen() {
        let c = conn();
        register(&c, "node-a", 1, "0.1.0", 1000).unwrap();
        heartbeat(&c, "node-a", 5000).unwrap();
        assert_eq!(get(&c, "node-a").unwrap().unwrap().last_seen_ms, 5000);
        assert!(heartbeat(&c, "missing", 5000).is_err());
    }

    // 1.04: 心跳超时后不再接受新分配。
    #[test]
    fn stale_node_stops_accepting() {
        let c = conn();
        register(&c, "node-a", 1, "0.1.0", 1000).unwrap();
        let n = get(&c, "node-a").unwrap().unwrap();
        assert!(is_accepting(&n, 1000 + 30_000, 30_000));
        assert!(!is_accepting(&n, 1000 + 30_001, 30_000));
    }
}
