//! 构建目标约束（plan.md 0.00a / 0.00b）。
//!
//! 固定每个组件的目标 OS/CPU 架构，拒绝缺少目标架构的构建配置，
//! 避免把开发机（arm64）的产物直接装进不同架构的镜像。

use std::collections::BTreeMap;

/// 单个组件的构建目标。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentTarget {
    /// 目标 CPU 架构，例如 "aarch64"、"x86_64"。必须显式指定。
    pub arch: String,
    /// 目标操作系统，例如 "linux"、"macos"。
    pub os: String,
}

/// 构建产物清单：记录每个组件的目标。
#[derive(Debug, Clone, Default)]
pub struct TargetManifest {
    components: BTreeMap<String, ComponentTarget>,
}

/// 目标校验错误。
#[derive(Debug, PartialEq, Eq)]
pub enum TargetError {
    /// 组件缺少目标架构配置。
    MissingArch { component: String },
    /// 组件缺少目标操作系统配置。
    MissingOs { component: String },
}

impl TargetManifest {
    pub fn new() -> Self {
        Self::default()
    }

    /// 登记一个组件的目标；arch/os 任一为空都视为未指定。
    pub fn set(&mut self, component: &str, arch: &str, os: &str) -> Result<(), TargetError> {
        if arch.is_empty() {
            return Err(TargetError::MissingArch {
                component: component.to_string(),
            });
        }
        if os.is_empty() {
            return Err(TargetError::MissingOs {
                component: component.to_string(),
            });
        }
        self.components.insert(
            component.to_string(),
            ComponentTarget {
                arch: arch.to_string(),
                os: os.to_string(),
            },
        );
        Ok(())
    }

    pub fn get(&self, component: &str) -> Option<&ComponentTarget> {
        self.components.get(component)
    }

    /// 清单里登记的组件名（有序）。
    pub fn components(&self) -> impl Iterator<Item = &String> {
        self.components.keys()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // 0.00a: 缺少目标架构的构建配置被拒绝。
    #[test]
    fn missing_arch_is_rejected() {
        let mut m = TargetManifest::new();
        let err = m.set("guest-kernel", "", "linux").unwrap_err();
        assert_eq!(
            err,
            TargetError::MissingArch {
                component: "guest-kernel".to_string()
            }
        );
        assert!(m.get("guest-kernel").is_none());
    }

    // 0.00b: 产物清单按组件记录目标 OS/CPU 架构。
    #[test]
    fn manifest_records_os_and_arch_per_component() {
        let mut m = TargetManifest::new();
        m.set("oiiaiod", "x86_64", "linux").unwrap();
        m.set("oiiaio-cli", "aarch64", "macos").unwrap();

        assert_eq!(m.get("oiiaiod").unwrap().arch, "x86_64");
        assert_eq!(m.get("oiiaiod").unwrap().os, "linux");
        assert_eq!(m.get("oiiaio-cli").unwrap().arch, "aarch64");

        let names: Vec<&String> = m.components().collect();
        assert_eq!(names, vec!["oiiaio-cli", "oiiaiod"]);
    }

    #[test]
    fn missing_os_is_rejected() {
        let mut m = TargetManifest::new();
        let err = m.set("rootfs", "x86_64", "").unwrap_err();
        assert_eq!(
            err,
            TargetError::MissingOs {
                component: "rootfs".to_string()
            }
        );
    }
}
