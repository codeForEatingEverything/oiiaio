//! oiiaio 核心库骨架。
//!
//! 当前仅含最小内容，用于让 workspace 可编译、可测试（plan.md 0.01–0.02）。

pub mod auth;
pub mod bench;
pub mod config;
pub mod fault;
pub mod target;
pub mod virt;

/// 返回构建标识，占位用，后续替换为真实的版本/能力协商逻辑。
pub fn build_id() -> &'static str {
    "oiiaio-dev"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_id_is_stable() {
        assert_eq!(build_id(), "oiiaio-dev");
    }
}
