//! 身份对象与单实例租约（plan.md 2.13/2.14/2.20/2.21/2.23 的纯逻辑部分）。
//!
//! 指纹/出口/浏览器实跑需 VM 与浏览器（阻塞）；这里实现不依赖它们的内核：
//! 身份对象持久化与导入、文件夹分组、单实例租约 + fencing（同一身份同一时间仅一个实例）。

use rusqlite::Connection;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    pub id: String,
    /// 文件夹分组（仅元数据，不改变隔离与权限）。
    pub group: String,
    /// 不透明配置（指纹/出口等的序列化），本内核不解释其内容。
    pub config_json: String,
}

#[derive(Debug, PartialEq, Eq)]
pub enum IdentityError {
    Store(String),
    /// 已有更高/相等 fence 的持有者，当前获取失败（2.20 单实例）。
    LeaseHeld {
        holder: String,
        fence: i64,
    },
    /// 使用了过期 fence（2.21）。
    StaleFence {
        current: i64,
        used: i64,
    },
    NotFound(String),
}

impl From<rusqlite::Error> for IdentityError {
    fn from(e: rusqlite::Error) -> Self {
        IdentityError::Store(e.to_string())
    }
}

pub fn init_schema(conn: &Connection) -> Result<(), IdentityError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS identities (
            id TEXT PRIMARY KEY,
            grp TEXT NOT NULL DEFAULT 'default',
            config_json TEXT NOT NULL
         );
         CREATE TABLE IF NOT EXISTS identity_leases (
            identity_id TEXT PRIMARY KEY,
            holder TEXT NOT NULL,
            fence INTEGER NOT NULL,
            active INTEGER NOT NULL
         );",
    )?;
    Ok(())
}

/// 创建身份（2.13）。
pub fn create(
    conn: &Connection,
    id: &str,
    group: &str,
    config_json: &str,
) -> Result<Identity, IdentityError> {
    conn.execute(
        "INSERT INTO identities (id, grp, config_json) VALUES (?1, ?2, ?3)",
        (id, group, config_json),
    )?;
    Ok(Identity {
        id: id.to_string(),
        group: group.to_string(),
        config_json: config_json.to_string(),
    })
}

pub fn get(conn: &Connection, id: &str) -> Result<Option<Identity>, IdentityError> {
    let mut stmt = conn.prepare("SELECT id, grp, config_json FROM identities WHERE id = ?1")?;
    let mut rows = stmt.query([id])?;
    match rows.next()? {
        Some(r) => Ok(Some(Identity {
            id: r.get(0)?,
            group: r.get(1)?,
            config_json: r.get(2)?,
        })),
        None => Ok(None),
    }
}

/// 导入身份：字段原样保留（2.14）。重复 id 视为更新。
pub fn import(conn: &Connection, ident: &Identity) -> Result<(), IdentityError> {
    conn.execute(
        "INSERT INTO identities (id, grp, config_json) VALUES (?1, ?2, ?3)
         ON CONFLICT(id) DO UPDATE SET grp = excluded.grp, config_json = excluded.config_json",
        (&ident.id, &ident.group, &ident.config_json),
    )?;
    Ok(())
}

/// 按分组列出（2.23：分组只是元数据）。
pub fn list_group(conn: &Connection, group: &str) -> Result<Vec<String>, IdentityError> {
    let mut stmt = conn.prepare("SELECT id FROM identities WHERE grp = ?1 ORDER BY id")?;
    let rows = stmt.query_map([group], |r| r.get::<_, String>(0))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// 当前租约（持有者, fence, 是否活跃）。
fn current_lease(
    conn: &Connection,
    identity_id: &str,
) -> Result<Option<(String, i64, bool)>, IdentityError> {
    let mut stmt =
        conn.prepare("SELECT holder, fence, active FROM identity_leases WHERE identity_id = ?1")?;
    let mut rows = stmt.query([identity_id])?;
    match rows.next()? {
        Some(r) => Ok(Some((r.get(0)?, r.get(1)?, r.get::<_, i64>(2)? != 0))),
        None => Ok(None),
    }
}

/// 获取单实例租约（2.20）：仅当无活跃租约时成功；成功则 fence 递增。
pub fn acquire(conn: &Connection, identity_id: &str, holder: &str) -> Result<i64, IdentityError> {
    if get(conn, identity_id)?.is_none() {
        return Err(IdentityError::NotFound(identity_id.to_string()));
    }
    match current_lease(conn, identity_id)? {
        Some((h, fence, true)) => Err(IdentityError::LeaseHeld { holder: h, fence }),
        Some((_, fence, false)) => {
            let next = fence + 1;
            conn.execute(
                "UPDATE identity_leases SET holder = ?1, fence = ?2, active = 1 WHERE identity_id = ?3",
                (holder, next, identity_id),
            )?;
            Ok(next)
        }
        None => {
            conn.execute(
                "INSERT INTO identity_leases (identity_id, holder, fence, active) VALUES (?1, ?2, 1, 1)",
                (identity_id, holder),
            )?;
            Ok(1)
        }
    }
}

/// 释放租约。
pub fn release(conn: &Connection, identity_id: &str, fence: i64) -> Result<(), IdentityError> {
    guard_fence(conn, identity_id, fence)?;
    conn.execute(
        "UPDATE identity_leases SET active = 0 WHERE identity_id = ?1",
        [identity_id],
    )?;
    Ok(())
}

/// fencing 校验（2.21）：只有持最新 fence 才能操作，旧 fence 被拒绝。
pub fn guard_fence(conn: &Connection, identity_id: &str, fence: i64) -> Result<(), IdentityError> {
    match current_lease(conn, identity_id)? {
        Some((_, current, _)) if fence == current => Ok(()),
        Some((_, current, _)) => Err(IdentityError::StaleFence {
            current,
            used: fence,
        }),
        None => Err(IdentityError::NotFound(identity_id.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conn() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        init_schema(&c).unwrap();
        c
    }

    // 2.13: 创建后可读回。
    #[test]
    fn create_and_get() {
        let c = conn();
        create(&c, "id-1", "default", "{\"tz\":\"UTC\"}").unwrap();
        assert_eq!(
            get(&c, "id-1").unwrap().unwrap().config_json,
            "{\"tz\":\"UTC\"}"
        );
    }

    // 2.14: 导入字段保持一致。
    #[test]
    fn import_keeps_fields() {
        let c = conn();
        let ident = Identity {
            id: "id-2".into(),
            group: "work".into(),
            config_json: "{\"a\":1}".into(),
        };
        import(&c, &ident).unwrap();
        assert_eq!(get(&c, "id-2").unwrap().unwrap(), ident);
    }

    // 2.23: 分组是元数据，可按组列出。
    #[test]
    fn grouping_lists_members() {
        let c = conn();
        create(&c, "a", "g1", "{}").unwrap();
        create(&c, "b", "g1", "{}").unwrap();
        create(&c, "c", "g2", "{}").unwrap();
        assert_eq!(list_group(&c, "g1").unwrap(), vec!["a", "b"]);
        assert_eq!(list_group(&c, "g2").unwrap(), vec!["c"]);
    }

    // 2.20: 同一身份同时只有一个活跃实例。
    #[test]
    fn single_instance_lease() {
        let c = conn();
        create(&c, "id-1", "default", "{}").unwrap();
        let f1 = acquire(&c, "id-1", "holder-A").unwrap();
        assert_eq!(f1, 1);
        // 第二个获取失败
        assert!(matches!(
            acquire(&c, "id-1", "holder-B"),
            Err(IdentityError::LeaseHeld { .. })
        ));
        // 释放后可被再次获取，fence 递增
        release(&c, "id-1", f1).unwrap();
        let f2 = acquire(&c, "id-1", "holder-B").unwrap();
        assert_eq!(f2, 2);
    }

    // 2.21: 旧 fence 不能继续操作。
    #[test]
    fn stale_fence_rejected() {
        let c = conn();
        create(&c, "id-1", "default", "{}").unwrap();
        let f1 = acquire(&c, "id-1", "A").unwrap();
        release(&c, "id-1", f1).unwrap();
        let _f2 = acquire(&c, "id-1", "B").unwrap();
        // 旧持有者用 f1 操作被拒绝
        assert!(matches!(
            guard_fence(&c, "id-1", f1),
            Err(IdentityError::StaleFence { .. })
        ));
        assert!(guard_fence(&c, "id-1", 2).is_ok());
    }

    #[test]
    fn acquire_unknown_identity_fails() {
        let c = conn();
        assert!(matches!(
            acquire(&c, "nope", "A"),
            Err(IdentityError::NotFound(_))
        ));
    }
}
