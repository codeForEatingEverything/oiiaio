//! 内部契约信封与协议版本校验（plan.md 1.00a / 1.00c）。
//!
//! 控制面、节点、CLI、SDK、WebApp 共用同一组请求/响应类型；
//! 每条消息带协议版本，不兼容版本被拒绝。
//! 说明：1.00b（由同一契约生成 TypeScript 类型/客户端）需额外代码生成工具链，
//! 不在本环境实现；此处先固定 Rust 侧权威类型与校验。

use serde::{Deserialize, Serialize};

/// 当前内部协议版本。
pub const PROTOCOL_VERSION: u32 = 1;

/// 统一消息信封。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Envelope<T> {
    pub protocol: u32,
    pub payload: T,
}

impl<T> Envelope<T> {
    pub fn new(payload: T) -> Self {
        Self {
            protocol: PROTOCOL_VERSION,
            payload,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum ContractError {
    /// 协议版本不兼容（1.00c）。
    IncompatibleProtocol { got: u32, want: u32 },
}

/// 校验收到的协议版本是否兼容。
pub fn check_protocol(got: u32) -> Result<(), ContractError> {
    if got == PROTOCOL_VERSION {
        Ok(())
    } else {
        Err(ContractError::IncompatibleProtocol {
            got,
            want: PROTOCOL_VERSION,
        })
    }
}

/// 示例资源请求：创建工作区（契约的一个具体样例）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateWorkspaceRequest {
    pub tenant: String,
    pub vcpus: i64,
    pub mem_mb: i64,
}

/// 示例资源响应。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateWorkspaceResponse {
    pub workspace_id: String,
    pub state: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    // 1.00a: 最小资源请求/响应通过同一契约（序列化往返一致）。
    #[test]
    fn request_response_roundtrip_through_envelope() {
        let req = Envelope::new(CreateWorkspaceRequest {
            tenant: "t1".into(),
            vcpus: 2,
            mem_mb: 2048,
        });
        let json = serde_json::to_string(&req).unwrap();
        let back: Envelope<CreateWorkspaceRequest> = serde_json::from_str(&json).unwrap();
        assert_eq!(back, req);
        assert_eq!(back.protocol, PROTOCOL_VERSION);

        let resp = Envelope::new(CreateWorkspaceResponse {
            workspace_id: "w1".into(),
            state: "pending".into(),
        });
        let rj = serde_json::to_string(&resp).unwrap();
        let rb: Envelope<CreateWorkspaceResponse> = serde_json::from_str(&rj).unwrap();
        assert_eq!(rb.payload.workspace_id, "w1");
    }

    // 1.00c: 不兼容协议版本被拒绝。
    #[test]
    fn incompatible_protocol_rejected() {
        assert!(check_protocol(PROTOCOL_VERSION).is_ok());
        assert_eq!(
            check_protocol(999),
            Err(ContractError::IncompatibleProtocol {
                got: 999,
                want: PROTOCOL_VERSION
            })
        );
    }
}
