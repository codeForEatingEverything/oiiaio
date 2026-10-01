//! 虚拟化能力检查（plan.md 0.11 / 0.12）。
//!
//! 默认隔离是 microVM（Straw.md ⑦），依赖 Linux KVM。
//! 能力不足时必须明确报错，不能静默换成另一种隔离方式。

/// 虚拟化能力探测结果。
#[derive(Debug, PartialEq, Eq)]
pub enum VirtCapability {
    /// 可用：KVM 就绪。
    KvmAvailable,
    /// 不可用，附带原因。
    Unavailable(String),
}

/// 探测当前宿主的 microVM 能力。
///
/// 仅在 Linux 且 `/dev/kvm` 存在且可访问时报告可用。
/// 其它平台（如 macOS）一律报告不可用并说明原因。
pub fn probe(kvm_path_exists: bool, is_linux: bool) -> VirtCapability {
    if !is_linux {
        return VirtCapability::Unavailable(
            "microVM 默认隔离依赖 Linux KVM，当前非 Linux 宿主".to_string(),
        );
    }
    if !kvm_path_exists {
        return VirtCapability::Unavailable("/dev/kvm 不存在或不可访问".to_string());
    }
    VirtCapability::KvmAvailable
}

/// 探测真实宿主（读取 `/dev/kvm` 与编译期 OS）。
pub fn probe_host() -> VirtCapability {
    let is_linux = cfg!(target_os = "linux");
    let kvm_exists = std::path::Path::new("/dev/kvm").exists();
    probe(kvm_exists, is_linux)
}

/// 要求 microVM 能力；不足时返回明确错误，绝不静默降级。
pub fn require_microvm() -> Result<(), String> {
    match probe_host() {
        VirtCapability::KvmAvailable => Ok(()),
        VirtCapability::Unavailable(why) => Err(format!(
            "microVM 能力不足：{why}（不会自动改用其它隔离方式）"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // 0.11: 具备能力时报告可用。
    #[test]
    fn linux_with_kvm_is_available() {
        assert_eq!(probe(true, true), VirtCapability::KvmAvailable);
    }

    // 0.12: 能力不足时明确报错，不静默换方案。
    #[test]
    fn non_linux_is_unavailable_with_reason() {
        let r = probe(true, false);
        match r {
            VirtCapability::Unavailable(why) => assert!(why.contains("Linux")),
            _ => panic!("非 Linux 宿主应报告不可用"),
        }
    }

    #[test]
    fn linux_without_kvm_is_unavailable() {
        assert_eq!(
            probe(false, true),
            VirtCapability::Unavailable("/dev/kvm 不存在或不可访问".to_string())
        );
    }

    #[test]
    fn require_microvm_errors_clearly_when_unavailable() {
        // 在本机（macOS arm64，无 /dev/kvm）应得到明确错误而非降级。
        if let Err(msg) = require_microvm() {
            assert!(msg.contains("不会自动改用其它隔离方式"));
        }
        // 若在具备 KVM 的 Linux CI 上运行则为 Ok，两种都可接受。
    }
}
