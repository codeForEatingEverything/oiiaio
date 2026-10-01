//! 鉴权主体与 scope（plan.md 1.00d–g）。
//!
//! 统一 token 的主体（租户/principal/类型）与 scope（资源+动作+有效期+audience）结构；
//! 基础鉴权之后进入策略拦截点（初版对已通过者返回 allow，第五阶段加规则）。
//! 说明：此处只做结构与边界校验，真正的签名/验签在后续接入。

/// 主体类型。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrincipalType {
    User,
    Agent,
    Node,
    Service,
}

/// 一条 scope：允许对某资源执行某动作。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scope {
    pub resource: String,
    pub action: String,
}

/// 解析后的 token 主体与权限。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub tenant: String,
    pub principal: String,
    pub principal_type: PrincipalType,
    pub scopes: Vec<Scope>,
    /// 过期时间（unix 秒）。
    pub expires_at: u64,
    pub audience: String,
}

#[derive(Debug, PartialEq, Eq)]
pub enum AuthError {
    Malformed(String),
    Expired,
    WrongAudience,
    /// 通过了身份验证，但越出租户/资源/scope 边界。
    Forbidden(String),
}

/// 一次访问请求。
pub struct AccessRequest<'a> {
    pub tenant: &'a str,
    pub resource: &'a str,
    pub action: &'a str,
    pub audience: &'a str,
}

/// 基础身份验证（0.00e）：结构完整、未过期、audience 正确。
pub fn authenticate(token: &Token, now: u64, expected_audience: &str) -> Result<(), AuthError> {
    if token.tenant.is_empty() || token.principal.is_empty() {
        return Err(AuthError::Malformed("缺少 tenant 或 principal".into()));
    }
    if token.expires_at <= now {
        return Err(AuthError::Expired);
    }
    if token.audience != expected_audience {
        return Err(AuthError::WrongAudience);
    }
    Ok(())
}

/// 基础授权边界（0.00f）：不能跨租户，且需命中一条 scope。
pub fn authorize(token: &Token, req: &AccessRequest) -> Result<(), AuthError> {
    if token.tenant != req.tenant {
        return Err(AuthError::Forbidden(format!(
            "跨租户访问被拒绝: token={} req={}",
            token.tenant, req.tenant
        )));
    }
    let hit = token
        .scopes
        .iter()
        .any(|s| s.resource == req.resource && s.action == req.action);
    if !hit {
        return Err(AuthError::Forbidden(format!(
            "无匹配 scope: {}:{}",
            req.resource, req.action
        )));
    }
    Ok(())
}

/// 策略拦截点（0.00g）。初版：对已通过基础鉴权的请求返回 allow。
/// 第五阶段在同一接口加载规则。
pub trait PolicyPoint {
    fn evaluate(&self, token: &Token, req: &AccessRequest) -> PolicyDecision;
}

#[derive(Debug, PartialEq, Eq)]
pub enum PolicyDecision {
    Allow,
    Deny(String),
}

/// 初版策略点：全部放行。
pub struct AllowAll;
impl PolicyPoint for AllowAll {
    fn evaluate(&self, _t: &Token, _r: &AccessRequest) -> PolicyDecision {
        PolicyDecision::Allow
    }
}

/// 完整准入：验证 → 授权 → 策略点。
pub fn admit(
    token: &Token,
    req: &AccessRequest,
    now: u64,
    policy: &dyn PolicyPoint,
) -> Result<(), AuthError> {
    authenticate(token, now, req.audience)?;
    authorize(token, req)?;
    match policy.evaluate(token, req) {
        PolicyDecision::Allow => Ok(()),
        PolicyDecision::Deny(why) => Err(AuthError::Forbidden(format!("策略拒绝: {why}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tok() -> Token {
        Token {
            tenant: "t1".into(),
            principal: "agent-1".into(),
            principal_type: PrincipalType::Agent,
            scopes: vec![Scope {
                resource: "workspace:w1".into(),
                action: "mcp.file.write".into(),
            }],
            expires_at: 1000,
            audience: "oiiaio-mcp".into(),
        }
    }
    fn req<'a>() -> AccessRequest<'a> {
        AccessRequest {
            tenant: "t1",
            resource: "workspace:w1",
            action: "mcp.file.write",
            audience: "oiiaio-mcp",
        }
    }

    // 1.00d: 主体与 scope 按统一结构解析可用。
    #[test]
    fn token_structure_parses() {
        let t = tok();
        assert_eq!(t.principal_type, PrincipalType::Agent);
        assert_eq!(t.scopes[0].action, "mcp.file.write");
    }

    // 1.00e: 过期/错 audience 被拒绝。
    #[test]
    fn expired_or_wrong_audience_rejected() {
        assert_eq!(
            authenticate(&tok(), 1000, "oiiaio-mcp"),
            Err(AuthError::Expired)
        );
        assert_eq!(
            authenticate(&tok(), 999, "other"),
            Err(AuthError::WrongAudience)
        );
        assert!(authenticate(&tok(), 999, "oiiaio-mcp").is_ok());
    }

    // 1.00f: 合法 token 仍不能跨租户或超出 scope。
    #[test]
    fn cannot_exceed_tenant_or_scope() {
        let cross = AccessRequest {
            tenant: "t2",
            ..req()
        };
        assert!(matches!(
            authorize(&tok(), &cross),
            Err(AuthError::Forbidden(_))
        ));
        let other_action = AccessRequest {
            action: "mcp.file.delete",
            ..req()
        };
        assert!(matches!(
            authorize(&tok(), &other_action),
            Err(AuthError::Forbidden(_))
        ));
        assert!(authorize(&tok(), &req()).is_ok());
    }

    // 1.00g: 通过基础鉴权后进入策略点，初版放行。
    #[test]
    fn admit_passes_through_policy_point() {
        assert!(admit(&tok(), &req(), 999, &AllowAll).is_ok());
        // 过期在策略点之前就被拦下
        assert_eq!(
            admit(&tok(), &req(), 1000, &AllowAll),
            Err(AuthError::Expired)
        );
    }
}
