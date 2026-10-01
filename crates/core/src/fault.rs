//! 故障注入夹具（plan.md 0.17a–c）。
//!
//! 供后续各阶段共用：在命名的操作步骤处注入故障（模拟进程被终止、
//! 连接被断开），并产出统一的故障报告（注入点、状态、清理结果）。
//! 这是测试支撑，不进入生产路径。

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};

/// 命名故障点的注入器：在某步骤 arm 后，`check` 返回注入的故障。
#[derive(Default)]
pub struct FaultInjector {
    armed: HashMap<String, String>,
}

/// 被注入的故障。
#[derive(Debug, PartialEq, Eq)]
pub struct InjectedFault {
    pub point: String,
    pub kind: String,
}

impl FaultInjector {
    pub fn new() -> Self {
        Self::default()
    }

    /// 在某故障点布设一种故障（如 "process_kill"、"net_drop"）。
    pub fn arm(&mut self, point: &str, kind: &str) {
        self.armed.insert(point.to_string(), kind.to_string());
    }

    /// 被测代码在关键步骤调用；若该点已布设则消费一次并返回故障。
    pub fn check(&mut self, point: &str) -> Result<(), InjectedFault> {
        if let Some(kind) = self.armed.remove(point) {
            Err(InjectedFault {
                point: point.to_string(),
                kind,
            })
        } else {
            Ok(())
        }
    }
}

/// 可断开/恢复的连接闸门（模拟网络故障）。
#[derive(Default)]
pub struct NetGate {
    up: AtomicBool,
}

impl NetGate {
    pub fn up() -> Self {
        Self {
            up: AtomicBool::new(true),
        }
    }
    pub fn disconnect(&self) {
        self.up.store(false, Ordering::SeqCst);
    }
    pub fn reconnect(&self) {
        self.up.store(true, Ordering::SeqCst);
    }
    /// 发送尝试：断开时失败。
    pub fn try_send(&self) -> Result<(), String> {
        if self.up.load(Ordering::SeqCst) {
            Ok(())
        } else {
            Err("连接已断开".to_string())
        }
    }
}

/// 统一故障报告（0.17c）。
#[derive(Debug, PartialEq, Eq)]
pub struct FaultReport {
    pub point: String,
    pub state_before: String,
    pub cleaned_up: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    // 0.17a: 测试进程能在指定操作步骤被“终止”（注入故障）。
    #[test]
    fn fault_fires_only_at_armed_point() {
        let mut fi = FaultInjector::new();
        fi.arm("after_persist_before_vm", "process_kill");
        assert!(fi.check("before_persist").is_ok());
        let err = fi.check("after_persist_before_vm").unwrap_err();
        assert_eq!(err.kind, "process_kill");
        // 只触发一次，恢复后同点不再注入
        assert!(fi.check("after_persist_before_vm").is_ok());
    }

    // 0.17b: 测试连接可被断开并恢复。
    #[test]
    fn net_gate_disconnect_and_recover() {
        let g = NetGate::up();
        assert!(g.try_send().is_ok());
        g.disconnect();
        assert!(g.try_send().is_err());
        g.reconnect();
        assert!(g.try_send().is_ok());
    }

    // 0.17c: 故障报告记录注入点、状态和清理结果。
    #[test]
    fn report_captures_point_state_cleanup() {
        let r = FaultReport {
            point: "snapshot_commit".to_string(),
            state_before: "preparing".to_string(),
            cleaned_up: true,
        };
        assert_eq!(r.point, "snapshot_commit");
        assert_eq!(r.state_before, "preparing");
        assert!(r.cleaned_up);
    }
}
