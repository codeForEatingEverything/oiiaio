//! 持久操作记录与状态机（plan.md 1.05a–e）。
//!
//! 控制面的每个操作（如创建工作区）在调用节点前先持久化；只允许合法状态转换；
//! 重启后能读到最后已提交步骤；同一幂等键返回同一操作；未完成操作可继续或补偿。
//! 第四阶段的持久执行（⑤）在此基础上扩展，不另起一套引擎。

use rusqlite::Connection;

use crate::StoreError;

/// 操作状态。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpState {
    Pending,
    Running,
    Committed,
    Failed,
}

impl OpState {
    fn as_str(&self) -> &'static str {
        match self {
            OpState::Pending => "pending",
            OpState::Running => "running",
            OpState::Committed => "committed",
            OpState::Failed => "failed",
        }
    }
    fn parse(s: &str) -> OpState {
        match s {
            "pending" => OpState::Pending,
            "running" => OpState::Running,
            "committed" => OpState::Committed,
            _ => OpState::Failed,
        }
    }
    /// 合法状态转换（1.05b）。
    fn can_transition_to(&self, next: &OpState) -> bool {
        matches!(
            (self, next),
            (OpState::Pending, OpState::Running)
                | (OpState::Running, OpState::Committed)
                | (OpState::Running, OpState::Failed)
                | (OpState::Pending, OpState::Failed)
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Operation {
    pub id: String,
    pub kind: String,
    pub idempotency_key: String,
    pub state: OpState,
    /// 最后已提交步骤序号。
    pub last_step: i64,
}

pub fn init_schema(conn: &Connection) -> Result<(), StoreError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS operations (
            id TEXT PRIMARY KEY,
            kind TEXT NOT NULL,
            idempotency_key TEXT NOT NULL UNIQUE,
            state TEXT NOT NULL,
            last_step INTEGER NOT NULL DEFAULT 0
        )",
    )?;
    Ok(())
}

/// 创建或返回已存在的操作（1.05a 先持久化；1.05d 幂等）。
pub fn begin(
    conn: &Connection,
    id: &str,
    kind: &str,
    idempotency_key: &str,
) -> Result<Operation, StoreError> {
    if let Some(existing) = get_by_key(conn, idempotency_key)? {
        return Ok(existing);
    }
    conn.execute(
        "INSERT INTO operations (id, kind, idempotency_key, state, last_step)
         VALUES (?1, ?2, ?3, 'pending', 0)",
        (id, kind, idempotency_key),
    )?;
    Ok(Operation {
        id: id.to_string(),
        kind: kind.to_string(),
        idempotency_key: idempotency_key.to_string(),
        state: OpState::Pending,
        last_step: 0,
    })
}

pub fn get(conn: &Connection, id: &str) -> Result<Option<Operation>, StoreError> {
    query_one(conn, "id", id)
}

pub fn get_by_key(conn: &Connection, key: &str) -> Result<Option<Operation>, StoreError> {
    query_one(conn, "idempotency_key", key)
}

fn query_one(conn: &Connection, col: &str, val: &str) -> Result<Option<Operation>, StoreError> {
    let sql = format!(
        "SELECT id, kind, idempotency_key, state, last_step FROM operations WHERE {col} = ?1"
    );
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query([val])?;
    match rows.next()? {
        Some(r) => Ok(Some(Operation {
            id: r.get(0)?,
            kind: r.get(1)?,
            idempotency_key: r.get(2)?,
            state: OpState::parse(&r.get::<_, String>(3)?),
            last_step: r.get(4)?,
        })),
        None => Ok(None),
    }
}

/// 推进状态；非法转换被拒绝（1.05b）。可同时推进 last_step（1.05c）。
pub fn transition(
    conn: &Connection,
    id: &str,
    next: OpState,
    step: Option<i64>,
) -> Result<(), StoreError> {
    let cur = get(conn, id)?.ok_or_else(|| StoreError::Backend(format!("操作不存在: {id}")))?;
    if !cur.state.can_transition_to(&next) {
        return Err(StoreError::Backend(format!(
            "非法状态转换: {:?} -> {:?}",
            cur.state, next
        )));
    }
    let step = step.unwrap_or(cur.last_step);
    conn.execute(
        "UPDATE operations SET state = ?1, last_step = ?2 WHERE id = ?3",
        (next.as_str(), step, id),
    )?;
    Ok(())
}

/// 列出未完成（pending/running）的操作，供重启后继续或补偿（1.05e）。
pub fn unfinished(conn: &Connection) -> Result<Vec<Operation>, StoreError> {
    let mut stmt = conn.prepare(
        "SELECT id, kind, idempotency_key, state, last_step FROM operations
         WHERE state IN ('pending','running') ORDER BY id",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(Operation {
            id: r.get(0)?,
            kind: r.get(1)?,
            idempotency_key: r.get(2)?,
            state: OpState::parse(&r.get::<_, String>(3)?),
            last_step: r.get(4)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conn() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        init_schema(&c).unwrap();
        c
    }

    // 1.05a: 调用节点前先持久化（begin 写入 pending）。
    #[test]
    fn begin_persists_pending() {
        let c = conn();
        let op = begin(&c, "op1", "workspace.create", "k1").unwrap();
        assert_eq!(op.state, OpState::Pending);
        assert_eq!(get(&c, "op1").unwrap().unwrap().state, OpState::Pending);
    }

    // 1.05d: 同一幂等键返回同一操作，不新建。
    #[test]
    fn idempotent_begin() {
        let c = conn();
        let a = begin(&c, "op1", "workspace.create", "k1").unwrap();
        let b = begin(&c, "op2", "workspace.create", "k1").unwrap();
        assert_eq!(a.id, b.id); // 仍是 op1
        assert_eq!(unfinished(&c).unwrap().len(), 1);
    }

    // 1.05b: 只允许合法状态转换。
    #[test]
    fn only_legal_transitions() {
        let c = conn();
        begin(&c, "op1", "k", "k1").unwrap();
        // pending -> committed 非法
        assert!(transition(&c, "op1", OpState::Committed, None).is_err());
        // pending -> running -> committed 合法
        transition(&c, "op1", OpState::Running, Some(1)).unwrap();
        transition(&c, "op1", OpState::Committed, Some(2)).unwrap();
        assert_eq!(get(&c, "op1").unwrap().unwrap().state, OpState::Committed);
    }

    // 1.05c: 能读到最后已提交步骤。
    #[test]
    fn records_last_step() {
        let c = conn();
        begin(&c, "op1", "k", "k1").unwrap();
        transition(&c, "op1", OpState::Running, Some(3)).unwrap();
        assert_eq!(get(&c, "op1").unwrap().unwrap().last_step, 3);
    }

    // 1.05e: 未完成操作可被枚举以继续或补偿。
    #[test]
    fn lists_unfinished_for_recovery() {
        let c = conn();
        begin(&c, "op1", "k", "k1").unwrap();
        begin(&c, "op2", "k", "k2").unwrap();
        transition(&c, "op2", OpState::Running, Some(1)).unwrap();
        transition(&c, "op1", OpState::Running, Some(1)).unwrap();
        transition(&c, "op1", OpState::Committed, Some(2)).unwrap();
        let u = unfinished(&c).unwrap();
        assert_eq!(u.len(), 1);
        assert_eq!(u[0].id, "op2");
    }
}
