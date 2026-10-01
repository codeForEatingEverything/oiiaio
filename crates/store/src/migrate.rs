//! 数据库迁移（plan.md 0.09a–d）。
//!
//! 顺序执行初始迁移；已执行版本记录在 `schema_migrations`；
//! 重复执行不重复改结构；失败不记为成功，并报告失败版本。

use rusqlite::Connection;

use crate::StoreError;

/// 一条迁移：版本号 + SQL。
pub struct Migration {
    pub version: i64,
    pub sql: &'static str,
}

fn ensure_version_table(conn: &Connection) -> Result<(), StoreError> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS schema_migrations (version INTEGER PRIMARY KEY)",
        [],
    )?;
    Ok(())
}

/// 当前已应用的最高版本（无则 0）。
pub fn current_version(conn: &Connection) -> Result<i64, StoreError> {
    ensure_version_table(conn)?;
    let v: i64 = conn.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
        [],
        |r| r.get(0),
    )?;
    Ok(v)
}

/// 按版本升序应用尚未执行的迁移。返回本次新应用的数量。
///
/// 每条迁移与版本登记在同一事务内提交：SQL 失败则整条回滚，
/// 不会把失败迁移记为已完成。
pub fn apply(conn: &mut Connection, migrations: &[Migration]) -> Result<usize, StoreError> {
    ensure_version_table(conn)?;
    let mut applied = 0usize;
    let mut ordered: Vec<&Migration> = migrations.iter().collect();
    ordered.sort_by_key(|m| m.version);
    let start = current_version(conn)?;
    for m in ordered {
        if m.version <= start {
            continue;
        }
        let tx = conn.transaction()?;
        tx.execute_batch(m.sql)
            .map_err(|e| StoreError::Backend(format!("迁移 v{} 失败: {e}", m.version)))?;
        tx.execute(
            "INSERT INTO schema_migrations (version) VALUES (?1)",
            [m.version],
        )?;
        tx.commit()?;
        applied += 1;
    }
    Ok(applied)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MetaStore;

    fn conn() -> Connection {
        Connection::open_in_memory().unwrap()
    }

    fn initial() -> Vec<Migration> {
        vec![
            Migration {
                version: 1,
                sql: "CREATE TABLE node (id TEXT PRIMARY KEY)",
            },
            Migration {
                version: 2,
                sql: "CREATE TABLE workspace (id TEXT PRIMARY KEY)",
            },
        ]
    }

    // 0.09a: 空库顺序执行初始迁移后可用。
    #[test]
    fn fresh_db_applies_initial_migrations() {
        let mut c = conn();
        let n = apply(&mut c, &initial()).unwrap();
        assert_eq!(n, 2);
        assert_eq!(current_version(&c).unwrap(), 2);
        // 表确实建好
        c.execute("INSERT INTO node (id) VALUES ('n1')", [])
            .unwrap();
    }

    // 0.09c: 重复执行迁移不会重复修改结构。
    #[test]
    fn apply_is_idempotent() {
        let mut c = conn();
        assert_eq!(apply(&mut c, &initial()).unwrap(), 2);
        assert_eq!(apply(&mut c, &initial()).unwrap(), 0);
        assert_eq!(current_version(&c).unwrap(), 2);
    }

    // 0.09b: 上一版库升级后原记录可读取。
    #[test]
    fn upgrade_preserves_existing_rows() {
        let mut c = conn();
        apply(&mut c, &initial()[..1]).unwrap(); // 只到 v1
        c.execute("INSERT INTO node (id) VALUES ('keep')", [])
            .unwrap();
        apply(&mut c, &initial()).unwrap(); // 升到 v2
        let got: String = c
            .query_row("SELECT id FROM node WHERE id='keep'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(got, "keep");
    }

    // 0.09d: 迁移失败不被记录为成功，并报告失败版本。
    #[test]
    fn failed_migration_is_not_recorded() {
        let mut c = conn();
        apply(&mut c, &initial()).unwrap();
        let bad = vec![Migration {
            version: 3,
            sql: "THIS IS NOT SQL",
        }];
        let err = apply(&mut c, &bad).unwrap_err();
        match err {
            StoreError::Backend(m) => assert!(m.contains("v3"), "应报告失败版本: {m}"),
        }
        // 版本未推进
        assert_eq!(current_version(&c).unwrap(), 2);
    }

    // 迁移器可与既有 MetaStore 共存（同库不冲突）。
    #[test]
    fn coexists_with_meta_store() {
        let s = MetaStore::open(":memory:").unwrap();
        s.put("k", "v").unwrap();
        assert_eq!(s.get("k").unwrap(), Some("v".to_string()));
    }
}
