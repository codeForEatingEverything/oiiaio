//! 最小配置加载（plan.md 0.06 / 0.07）。
//!
//! 以控制面数据库为唯一真相源（见 Straw.md ⑯），TOML 是其投影。
//! 这里先实现最小的 TOML 解析与明确的错误反馈，后续再扩展字段。

use serde::Deserialize;

/// 最小配置：先只放一个节点名占位，后续按 §4 扩展。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Config {
    /// 节点名称。
    pub node_name: String,
}

/// 配置加载错误。
#[derive(Debug)]
pub enum ConfigError {
    /// TOML 解析失败，附带可读信息。
    Parse(String),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::Parse(msg) => write!(f, "配置解析失败: {msg}"),
        }
    }
}

impl std::error::Error for ConfigError {}

/// 从 TOML 文本加载配置。
pub fn load_from_str(input: &str) -> Result<Config, ConfigError> {
    toml::from_str(input).map_err(|e| ConfigError::Parse(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    // 0.06: 最小有效 TOML 能被读取。
    #[test]
    fn valid_minimal_toml_loads() {
        let cfg = load_from_str(r#"node_name = "node-a""#).expect("应能解析");
        assert_eq!(
            cfg,
            Config {
                node_name: "node-a".to_string()
            }
        );
    }

    // 0.07: 无效 TOML 返回明确错误。
    #[test]
    fn invalid_toml_returns_clear_error() {
        let err = load_from_str("node_name = ").unwrap_err();
        let ConfigError::Parse(msg) = err;
        assert!(!msg.is_empty(), "错误信息不应为空");
        assert_ne!(msg, "not implemented", "应为真实解析错误而非桩");
    }
}
