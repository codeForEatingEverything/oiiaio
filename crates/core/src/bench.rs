//! Benchmark 结果框架（plan.md 0.13–0.16）。
//!
//! 每次结果落盘，带版本/环境/样本数；同环境才允许直接比较；
//! 退化超阈值则 CI 门禁失败；缺基线或缺阈值要明确报告。

use serde::{Deserialize, Serialize};

/// 运行环境指纹：不同环境/架构的结果不可直接比较。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BenchEnv {
    pub os: String,
    pub arch: String,
    pub toolchain: String,
}

/// 一次 benchmark 的结果。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BenchResult {
    pub name: String,
    pub env: BenchEnv,
    pub sample_count: usize,
    pub p50_ms: f64,
    pub p95_ms: f64,
    /// 保留原始样本，便于复核与保留失败样本。
    pub raw_ms: Vec<f64>,
}

#[derive(Debug)]
pub enum BenchError {
    Io(String),
    /// 环境不同，结果不可直接比较（0.15）。
    EnvMismatch(Box<(BenchEnv, BenchEnv)>),
    /// 缺少基线或阈值（0.16b）。
    MissingBaseline,
    MissingThreshold,
}

impl std::fmt::Display for BenchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BenchError::Io(m) => write!(f, "benchmark IO 错误: {m}"),
            BenchError::EnvMismatch(pair) => write!(
                f,
                "环境不同，结果不可直接比较: baseline={:?} candidate={:?}",
                pair.0, pair.1
            ),
            BenchError::MissingBaseline => write!(f, "缺少基线，无法比较"),
            BenchError::MissingThreshold => write!(f, "缺少退化阈值，无法执行门禁"),
        }
    }
}
impl std::error::Error for BenchError {}

/// 把结果写成 JSON 文件（0.13）。
pub fn write_result(path: &str, r: &BenchResult) -> Result<(), BenchError> {
    let json = serde_json::to_string_pretty(r).map_err(|e| BenchError::Io(e.to_string()))?;
    std::fs::write(path, json).map_err(|e| BenchError::Io(e.to_string()))
}

/// 读回结果（0.13/0.14）。
pub fn read_result(path: &str) -> Result<BenchResult, BenchError> {
    let data = std::fs::read_to_string(path).map_err(|e| BenchError::Io(e.to_string()))?;
    serde_json::from_str(&data).map_err(|e| BenchError::Io(e.to_string()))
}

/// 同环境下 p95 的变化比例（candidate/baseline - 1）。异环境报错（0.14/0.15）。
pub fn p95_delta_ratio(baseline: &BenchResult, candidate: &BenchResult) -> Result<f64, BenchError> {
    if baseline.env != candidate.env {
        return Err(BenchError::EnvMismatch(Box::new((
            baseline.env.clone(),
            candidate.env.clone(),
        ))));
    }
    Ok(candidate.p95_ms / baseline.p95_ms - 1.0)
}

/// 退化门禁（0.16a/0.16b）：candidate 相对 baseline 的 p95 退化超过 `threshold`（如 0.10 = 10%）即失败。
pub fn regression_gate(
    baseline: Option<&BenchResult>,
    candidate: &BenchResult,
    threshold: Option<f64>,
) -> Result<bool, BenchError> {
    let baseline = baseline.ok_or(BenchError::MissingBaseline)?;
    let threshold = threshold.ok_or(BenchError::MissingThreshold)?;
    let delta = p95_delta_ratio(baseline, candidate)?;
    Ok(delta <= threshold)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env() -> BenchEnv {
        BenchEnv {
            os: "linux".into(),
            arch: "x86_64".into(),
            toolchain: "1.97.1".into(),
        }
    }
    fn result(name: &str, p95: f64, e: BenchEnv) -> BenchResult {
        BenchResult {
            name: name.into(),
            env: e,
            sample_count: 3,
            p50_ms: p95 * 0.6,
            p95_ms: p95,
            raw_ms: vec![p95 * 0.5, p95 * 0.6, p95],
        }
    }

    // 0.13: 结果能写文件并带版本/环境/样本数。
    #[test]
    fn write_then_read_keeps_env_and_samples() {
        let mut path = std::env::temp_dir();
        path.push(format!("oiiaio-bench-{}.json", std::process::id()));
        let p = path.to_str().unwrap();
        let r = result("workspace_create", 100.0, env());
        write_result(p, &r).unwrap();
        let back = read_result(p).unwrap();
        assert_eq!(back.env, env());
        assert_eq!(back.sample_count, 3);
        assert_eq!(back, r);
        let _ = std::fs::remove_file(p);
    }

    // 0.14: 同环境可计算差异。
    #[test]
    fn same_env_diff() {
        let d = p95_delta_ratio(&result("x", 100.0, env()), &result("x", 110.0, env())).unwrap();
        assert!((d - 0.10).abs() < 1e-9);
    }

    // 0.15: 不同架构/环境标记为不可直接比较。
    #[test]
    fn different_env_is_not_comparable() {
        let other = BenchEnv {
            arch: "aarch64".into(),
            ..env()
        };
        let err =
            p95_delta_ratio(&result("x", 100.0, env()), &result("x", 100.0, other)).unwrap_err();
        assert!(matches!(err, BenchError::EnvMismatch(_)));
    }

    // 0.16a: 超过阈值门禁失败，未超过则通过。
    #[test]
    fn regression_gate_threshold() {
        let base = result("x", 100.0, env());
        let bad = result("x", 130.0, env()); // +30%
        let ok = result("x", 105.0, env()); // +5%
        assert!(!regression_gate(Some(&base), &bad, Some(0.10)).unwrap());
        assert!(regression_gate(Some(&base), &ok, Some(0.10)).unwrap());
    }

    // 0.16b: 缺基线或缺阈值要明确报告。
    #[test]
    fn missing_baseline_or_threshold_is_reported() {
        let cand = result("x", 100.0, env());
        assert!(matches!(
            regression_gate(None, &cand, Some(0.1)).unwrap_err(),
            BenchError::MissingBaseline
        ));
        assert!(matches!(
            regression_gate(Some(&cand), &cand, None).unwrap_err(),
            BenchError::MissingThreshold
        ));
    }
}
