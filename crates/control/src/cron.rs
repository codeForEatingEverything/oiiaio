//! 按身份/触发器时区的定时逻辑（plan.md 4.30 / 4.31 的纯逻辑部分）。
//!
//! 实际调度器常驻与唤醒需运行环境；这里实现可测的时间计算：
//! 按指定 IANA 时区计算每日 HH:MM 的下一次触发（处理夏令时），
//! 以及"错过触发"的处置策略决策。

use chrono::{DateTime, Duration, TimeZone, Utc};
use chrono_tz::Tz;

/// 计算某时区每日 (hour:minute) 在 `after`(UTC) 之后的下一次触发时刻（UTC）。
/// 跨夏令时的本地时刻会被正确换算；遇到不存在/重复的本地时刻时顺延到有效时刻。
pub fn next_daily_fire(tz: Tz, hour: u32, minute: u32, after: DateTime<Utc>) -> DateTime<Utc> {
    let mut day = after.with_timezone(&tz).date_naive();
    for _ in 0..8 {
        let naive = day.and_hms_opt(hour, minute, 0).expect("有效 HH:MM");
        match tz.from_local_datetime(&naive) {
            chrono::LocalResult::Single(local) => {
                let utc = local.with_timezone(&Utc);
                if utc > after {
                    return utc;
                }
            }
            chrono::LocalResult::Ambiguous(a, _) => {
                let utc = a.with_timezone(&Utc);
                if utc > after {
                    return utc;
                }
            }
            chrono::LocalResult::None => {
                // 夏令时跳跃：该本地时刻不存在，顺延找当天稍后的有效时刻。
                for add in 1..=120 {
                    let n2 = day.and_hms_opt(hour, minute, 0).unwrap() + Duration::minutes(add);
                    if let chrono::LocalResult::Single(local) = tz.from_local_datetime(&n2) {
                        let utc = local.with_timezone(&Utc);
                        if utc > after {
                            return utc;
                        }
                    }
                }
            }
        }
        day = day.succ_opt().unwrap();
    }
    after
}

/// 错过触发的处置策略（4.31）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MissedPolicy {
    /// 开机后补跑一次。
    RunOnce,
    /// 全部补跑。
    RunAll,
    /// 跳过。
    Skip,
}

/// 依据策略计算应补跑的次数。
pub fn missed_runs(policy: MissedPolicy, missed: u32) -> u32 {
    match policy {
        MissedPolicy::RunOnce => missed.min(1),
        MissedPolicy::RunAll => missed,
        MissedPolicy::Skip => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono_tz::America::New_York;
    use chrono_tz::Asia::Tokyo;

    // 4.30: 按触发器时区触发（东京 09:00 → UTC 00:00）。
    #[test]
    fn fires_in_trigger_timezone() {
        let after = Utc.with_ymd_and_hms(2026, 3, 10, 12, 0, 0).unwrap();
        let fire = next_daily_fire(Tokyo, 9, 0, after);
        // 东京 UTC+9，次日 09:00 JST == 当日/次日 00:00 UTC
        assert_eq!(fire, Utc.with_ymd_and_hms(2026, 3, 11, 0, 0, 0).unwrap());
    }

    // 4.31: 跨越夏令时（美东 3 月 DST，本地 02:30 不存在时顺延）。
    #[test]
    fn handles_dst_spring_forward() {
        // 2026-03-08 美东夏令时：02:00→03:00，本地 02:30 不存在。
        let after = Utc.with_ymd_and_hms(2026, 3, 8, 5, 0, 0).unwrap();
        let fire = next_daily_fire(New_York, 2, 30, after);
        // 应顺延到存在的时刻（03:xx EDT = 07:xx UTC），不 panic、且在 after 之后。
        assert!(fire > after);
    }

    // 4.31: 错过触发策略。
    #[test]
    fn missed_policy_counts() {
        assert_eq!(missed_runs(MissedPolicy::RunOnce, 5), 1);
        assert_eq!(missed_runs(MissedPolicy::RunAll, 5), 5);
        assert_eq!(missed_runs(MissedPolicy::Skip, 5), 0);
    }
}
