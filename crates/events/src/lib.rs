//! 事件总线内核（plan.md 4.25–4.29 的纯逻辑部分）。
//!
//! 实际唤醒工作区/Runtime（4.37 等）需运行环境（阻塞）；这里实现：
//! 持久接收、按事件 id 去重、并发消费不重复、配置路由、未匹配转 LLM 路由。

use rusqlite::Connection;

#[derive(Debug, PartialEq, Eq)]
pub enum EventError {
    Store(String),
}
impl From<rusqlite::Error> for EventError {
    fn from(e: rusqlite::Error) -> Self {
        EventError::Store(e.to_string())
    }
}

pub struct EventBus {
    conn: Connection,
}

impl EventBus {
    pub fn open(path: &str) -> Result<Self, EventError> {
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS events (
                id TEXT PRIMARY KEY, kind TEXT NOT NULL, source TEXT NOT NULL,
                claimed_by TEXT, done INTEGER NOT NULL DEFAULT 0);",
        )?;
        Ok(Self { conn })
    }

    /// 持久接收一个事件，按 id 去重（4.25/4.26）。返回是否为新事件。
    pub fn accept(&self, id: &str, kind: &str, source: &str) -> Result<bool, EventError> {
        let n = self.conn.execute(
            "INSERT OR IGNORE INTO events (id, kind, source) VALUES (?1, ?2, ?3)",
            (id, kind, source),
        )?;
        Ok(n == 1)
    }

    /// 并发消费：原子认领一个未认领、未完成的事件（4.27）。同一事件只会被一个消费者拿到。
    pub fn claim_one(&self, consumer: &str) -> Result<Option<String>, EventError> {
        let tx = self.conn.unchecked_transaction()?;
        let id: Option<String> = tx
            .query_row(
                "SELECT id FROM events WHERE claimed_by IS NULL AND done = 0 ORDER BY id LIMIT 1",
                [],
                |r| r.get(0),
            )
            .ok();
        if let Some(ref id) = id {
            tx.execute(
                "UPDATE events SET claimed_by = ?1 WHERE id = ?2",
                (consumer, id),
            )?;
        }
        tx.commit()?;
        Ok(id)
    }

    pub fn claimed_by(&self, id: &str) -> Result<Option<String>, EventError> {
        Ok(self
            .conn
            .query_row("SELECT claimed_by FROM events WHERE id = ?1", [id], |r| {
                r.get(0)
            })
            .ok()
            .flatten())
    }
}

/// 路由规则：事件 kind 命中则指向某 agent。
pub struct Rule {
    pub kind: String,
    pub agent: String,
}

#[derive(Debug, PartialEq, Eq)]
pub enum RouteOutcome {
    /// 命中配置规则（4.28）。
    Agent(String),
    /// 未命中，转交 LLM 路由判断（4.29）。
    FallbackLlm,
}

/// 先按配置规则匹配，未匹配则转 LLM（4.28/4.29）。
pub fn route(kind: &str, rules: &[Rule]) -> RouteOutcome {
    match rules.iter().find(|r| r.kind == kind) {
        Some(r) => RouteOutcome::Agent(r.agent.clone()),
        None => RouteOutcome::FallbackLlm,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bus() -> EventBus {
        EventBus::open(":memory:").unwrap()
    }

    // 4.25: 持久接收一个事件。
    #[test]
    fn accept_persists() {
        let b = bus();
        assert!(b.accept("e1", "cron", "sched").unwrap());
    }

    // 4.26: 重复事件去重。
    #[test]
    fn duplicate_deduped() {
        let b = bus();
        assert!(b.accept("e1", "cron", "s").unwrap());
        assert!(!b.accept("e1", "cron", "s").unwrap());
    }

    // 4.27: 并发消费者不重复处理同一事件。
    #[test]
    fn claim_is_exclusive() {
        let b = bus();
        b.accept("e1", "k", "s").unwrap();
        let first = b.claim_one("c1").unwrap();
        assert_eq!(first.as_deref(), Some("e1"));
        // 第二个消费者拿不到（已被认领）
        assert_eq!(b.claim_one("c2").unwrap(), None);
        assert_eq!(b.claimed_by("e1").unwrap().as_deref(), Some("c1"));
    }

    // 4.28: 配置规则命中目标 agent。
    #[test]
    fn config_rule_routes() {
        let rules = vec![Rule {
            kind: "email.received".into(),
            agent: "inbox-agent".into(),
        }];
        assert_eq!(
            route("email.received", &rules),
            RouteOutcome::Agent("inbox-agent".into())
        );
    }

    // 4.29: 未匹配转 LLM 路由。
    #[test]
    fn unmatched_falls_back_to_llm() {
        let rules = vec![Rule {
            kind: "email.received".into(),
            agent: "x".into(),
        }];
        assert_eq!(route("webhook.unknown", &rules), RouteOutcome::FallbackLlm);
    }
}
