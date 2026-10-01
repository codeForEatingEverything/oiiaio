//! 最小调度（plan.md 1.07）。
//!
//! 纯逻辑：给定在线且有空闲容量的节点，为待创建工作区选一个。
//! 实际放置到宿主、启动 VM 需要 KVM 环境，另行实现。

/// 候选节点的可用容量。
#[derive(Debug, Clone)]
pub struct NodeCapacity {
    pub node_id: String,
    pub online: bool,
    pub free_vcpus: i64,
    pub free_mem_mb: i64,
}

/// 为需要 (vcpus, mem_mb) 的工作区选一个在线且容量足够的节点；
/// 选择 free_vcpus 最少但仍满足的（best-fit，减少碎片）。
pub fn pick_node(
    candidates: &[NodeCapacity],
    need_vcpus: i64,
    need_mem_mb: i64,
) -> Option<&NodeCapacity> {
    candidates
        .iter()
        .filter(|n| n.online && n.free_vcpus >= need_vcpus && n.free_mem_mb >= need_mem_mb)
        .min_by_key(|n| (n.free_vcpus, n.free_mem_mb))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(id: &str, online: bool, cpu: i64, mem: i64) -> NodeCapacity {
        NodeCapacity {
            node_id: id.into(),
            online,
            free_vcpus: cpu,
            free_mem_mb: mem,
        }
    }

    // 1.07: 待创建工作区能分配给可用节点。
    #[test]
    fn picks_an_available_node() {
        let cands = vec![n("a", true, 2, 2048), n("b", true, 8, 8192)];
        let chosen = pick_node(&cands, 2, 2048).unwrap();
        assert_eq!(chosen.node_id, "a"); // best-fit
    }

    #[test]
    fn skips_offline_and_insufficient() {
        let cands = vec![n("off", false, 16, 16384), n("small", true, 1, 512)];
        assert!(pick_node(&cands, 2, 2048).is_none());
    }
}
