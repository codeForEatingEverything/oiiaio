//! Trace 上下文传递（plan.md 1.00h）。
//!
//! 控制面新操作分配一个 trace id；派生的子请求携带同一 trace id、各自的 span id
//! 与父 span。跨进程（节点、guest、MCP）传递时序列化为头部键值。

/// 一次调用链的上下文。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceContext {
    pub trace_id: String,
    pub span_id: String,
    pub parent_span: Option<String>,
}

impl TraceContext {
    /// 新根上下文（操作入口）。
    pub fn root(trace_id: &str, span_id: &str) -> Self {
        Self {
            trace_id: trace_id.to_string(),
            span_id: span_id.to_string(),
            parent_span: None,
        }
    }

    /// 派生子上下文：保持 trace_id，新 span，父为当前 span。
    pub fn child(&self, child_span: &str) -> Self {
        Self {
            trace_id: self.trace_id.clone(),
            span_id: child_span.to_string(),
            parent_span: Some(self.span_id.clone()),
        }
    }

    /// 序列化为可随请求携带的头部键值。
    pub fn to_headers(&self) -> Vec<(String, String)> {
        let mut h = vec![
            ("x-oiiaio-trace".to_string(), self.trace_id.clone()),
            ("x-oiiaio-span".to_string(), self.span_id.clone()),
        ];
        if let Some(p) = &self.parent_span {
            h.push(("x-oiiaio-parent".to_string(), p.clone()));
        }
        h
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // 1.00h: 子请求携带同一 trace id，span 形成父子链。
    #[test]
    fn child_keeps_trace_id() {
        let root = TraceContext::root("trace-1", "span-a");
        let child = root.child("span-b");
        assert_eq!(child.trace_id, "trace-1");
        assert_eq!(child.parent_span.as_deref(), Some("span-a"));
        assert_ne!(child.span_id, root.span_id);
    }

    #[test]
    fn headers_carry_trace() {
        let h = TraceContext::root("t", "s").child("s2").to_headers();
        assert!(h.contains(&("x-oiiaio-trace".to_string(), "t".to_string())));
        assert!(h.contains(&("x-oiiaio-parent".to_string(), "s".to_string())));
    }
}
