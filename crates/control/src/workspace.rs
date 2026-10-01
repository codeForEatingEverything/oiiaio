//! 工作区元数据与配额（plan.md 1.05 / 1.06）。
//!
//! 仅数据逻辑：登记工作区配置、按租户资源上限判断是否放行。
//! 宿主上 cgroup 的真实限制（1.06a/b）需 Linux 宿主，另行实现。

use rusqlite::Connection;

use crate::node::NodeError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceSpec {
    pub id: String,
    pub tenant: String,
    pub vcpus: i64,
    pub mem_mb: i64,
}

/// 租户资源上限。
#[derive(Debug, Clone, Copy)]
pub struct TenantQuota {
    pub max_vcpus: i64,
    pub max_mem_mb: i64,
}

pub fn init_schema(conn: &Connection) -> Result<(), NodeError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS workspaces (
            id TEXT PRIMARY KEY,
            tenant TEXT NOT NULL,
            vcpus INTEGER NOT NULL,
            mem_mb INTEGER NOT NULL
        )",
    )?;
    Ok(())
}

fn tenant_usage(conn: &Connection, tenant: &str) -> Result<(i64, i64), NodeError> {
    let row = conn.query_row(
        "SELECT COALESCE(SUM(vcpus),0), COALESCE(SUM(mem_mb),0) FROM workspaces WHERE tenant = ?1",
        [tenant],
        |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)),
    )?;
    Ok(row)
}

/// 创建工作区；超出租户上限则拒绝（1.06）。成功后可查询（1.05）。
pub fn create(
    conn: &Connection,
    spec: &WorkspaceSpec,
    quota: TenantQuota,
) -> Result<(), NodeError> {
    let (used_cpu, used_mem) = tenant_usage(conn, &spec.tenant)?;
    if used_cpu + spec.vcpus > quota.max_vcpus || used_mem + spec.mem_mb > quota.max_mem_mb {
        return Err(NodeError::Store(format!(
            "超出租户资源上限: cpu {}+{}/{}, mem {}+{}/{}",
            used_cpu, spec.vcpus, quota.max_vcpus, used_mem, spec.mem_mb, quota.max_mem_mb
        )));
    }
    conn.execute(
        "INSERT INTO workspaces (id, tenant, vcpus, mem_mb) VALUES (?1, ?2, ?3, ?4)",
        (&spec.id, &spec.tenant, spec.vcpus, spec.mem_mb),
    )?;
    Ok(())
}

pub fn get(conn: &Connection, id: &str) -> Result<Option<WorkspaceSpec>, NodeError> {
    let mut stmt =
        conn.prepare("SELECT id, tenant, vcpus, mem_mb FROM workspaces WHERE id = ?1")?;
    let mut rows = stmt.query([id])?;
    match rows.next()? {
        Some(r) => Ok(Some(WorkspaceSpec {
            id: r.get(0)?,
            tenant: r.get(1)?,
            vcpus: r.get(2)?,
            mem_mb: r.get(3)?,
        })),
        None => Ok(None),
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
    fn quota() -> TenantQuota {
        TenantQuota {
            max_vcpus: 4,
            max_mem_mb: 4096,
        }
    }
    fn spec(id: &str, cpu: i64, mem: i64) -> WorkspaceSpec {
        WorkspaceSpec {
            id: id.into(),
            tenant: "t1".into(),
            vcpus: cpu,
            mem_mb: mem,
        }
    }

    // 1.05: 创建后可查询。
    #[test]
    fn create_then_query() {
        let c = conn();
        create(&c, &spec("w1", 2, 2048), quota()).unwrap();
        assert_eq!(get(&c, "w1").unwrap().unwrap().vcpus, 2);
    }

    // 1.06: 超出上限被拒绝，且不写入。
    #[test]
    fn over_quota_rejected() {
        let c = conn();
        create(&c, &spec("w1", 3, 2048), quota()).unwrap();
        let err = create(&c, &spec("w2", 2, 1024), quota()).unwrap_err(); // 3+2 > 4
        assert!(matches!(err, NodeError::Store(_)));
        assert!(get(&c, "w2").unwrap().is_none());
        // 内存维度同理
        assert!(create(&c, &spec("w3", 1, 4096), quota()).is_err());
    }
}
