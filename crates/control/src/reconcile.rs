//! 资源对账差异判定（plan.md 1.13b/c 的纯逻辑部分）。
//!
//! 枚举宿主真实资源（VM/tap/IP/磁盘）需要在宿主上进行（需 Linux/KVM，另行实现）。
//! 这里只做"持久记录 vs 实际清单"的差异与清理决策：
//! - 实际存在但记录中无主、且不在创建中 → 可清理的孤儿（1.13c：只清确认无主的）。
//! - 记录存在但实际缺失 → 需修正状态并进入恢复（1.13d，相反方向）。

use std::collections::BTreeSet;

/// 一次对账的输入。
pub struct ReconcileInput {
    /// 实际在宿主上存在的资源 id。
    pub actual: BTreeSet<String>,
    /// 记录中处于稳定态（运行/已提交）的资源 id。
    pub recorded_active: BTreeSet<String>,
    /// 记录中仍在创建中的资源 id（不可当孤儿清理）。
    pub recorded_in_flight: BTreeSet<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct ReconcilePlan {
    /// 确认无主、可清理的实际资源。
    pub orphans_to_clean: Vec<String>,
    /// 记录有但实际缺失，需修正并恢复。
    pub missing_to_recover: Vec<String>,
}

pub fn reconcile(input: &ReconcileInput) -> ReconcilePlan {
    // 孤儿：实际存在，但既不在稳定记录、也不在创建中。
    let orphans = input
        .actual
        .iter()
        .filter(|id| {
            !input.recorded_active.contains(*id) && !input.recorded_in_flight.contains(*id)
        })
        .cloned()
        .collect();
    // 缺失：稳定记录里有，但实际不存在。
    let missing = input
        .recorded_active
        .iter()
        .filter(|id| !input.actual.contains(*id))
        .cloned()
        .collect();
    ReconcilePlan {
        orphans_to_clean: orphans,
        missing_to_recover: missing,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(items: &[&str]) -> BTreeSet<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    // 1.13b/c: 识别差异，但只把确认无主（且非创建中）的实际资源列为可清理。
    #[test]
    fn orphans_exclude_in_flight() {
        let plan = reconcile(&ReconcileInput {
            actual: set(&["vm-a", "vm-orphan", "vm-creating"]),
            recorded_active: set(&["vm-a"]),
            recorded_in_flight: set(&["vm-creating"]),
        });
        assert_eq!(plan.orphans_to_clean, vec!["vm-orphan".to_string()]);
        assert!(plan.missing_to_recover.is_empty());
    }

    // 1.13d 方向：记录有而实际缺失 → 需恢复。
    #[test]
    fn detects_missing_recorded() {
        let plan = reconcile(&ReconcileInput {
            actual: set(&["vm-a"]),
            recorded_active: set(&["vm-a", "vm-gone"]),
            recorded_in_flight: set(&[]),
        });
        assert_eq!(plan.missing_to_recover, vec!["vm-gone".to_string()]);
        assert!(plan.orphans_to_clean.is_empty());
    }

    // 一切一致时无动作。
    #[test]
    fn no_action_when_consistent() {
        let plan = reconcile(&ReconcileInput {
            actual: set(&["vm-a", "vm-b"]),
            recorded_active: set(&["vm-a", "vm-b"]),
            recorded_in_flight: set(&[]),
        });
        assert!(plan.orphans_to_clean.is_empty() && plan.missing_to_recover.is_empty());
    }
}
