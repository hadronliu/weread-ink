//! 微信读书 Agent API Gateway 客户端。
//! 接口定义来自官方 Skill 文档（Tencent/WeChatReading, skill_version 1.0.5）。
use crate::{DayStat, Error, ReadingBook, Result, WeekStats};
use chrono::{Datelike, DateTime, Duration, Local, TimeZone};
use serde_json::{json, Value};
use std::time::Duration as StdDuration;

const GATEWAY: &str = "https://i.weread.qq.com/api/agent/gateway";
const SKILL_VERSION: &str = "1.0.5";
/// 单个网关请求超时；整个取数流程最多 3 次串行调用。
const CALL_TIMEOUT: StdDuration = StdDuration::from_secs(10);

pub(crate) struct Gateway {
    http: reqwest::Client,
    base: String,
    key: String,
}
impl Gateway {
    pub(crate) fn new(config: &crate::Config) -> Result<Self> {
        if config.api_key.trim().is_empty() {
            return Err(Error::NoApiKey);
        }
        Ok(Self {
            http: reqwest::Client::builder().timeout(CALL_TIMEOUT).build()?,
            base: config.base_url.clone().unwrap_or_else(|| GATEWAY.into()),
            key: config.api_key.clone(),
        })
    }

    /// 统一调用：平铺业务参数 + skill_version；处理 upgrade_info / errcode / data 信封。
    async fn call(&self, api: &str, params: Value) -> Result<Value> {
        let mut body = params;
        body["api_name"] = json!(api);
        body["skill_version"] = json!(SKILL_VERSION);
        let response: Value = self
            .http
            .post(&self.base)
            .bearer_auth(&self.key)
            .json(&body)
            .send()
            .await?
            .json()
            .await?;
        envelope(response)
    }

    /// `/readdata/detail` mode=weekly → 本周每日时长。
    pub(crate) async fn weekly(&self) -> Result<WeekStats> {
        let payload = self.call("/readdata/detail", json!({ "mode": "weekly" })).await?;
        parse_weekly(&payload)
    }

    /// 最近在读：书架按 readUpdateTime 倒序，过滤已读完和 `active_days` 天外未读的，
    /// 逐本查进度（最多 `max` 本），进度 100% 的丢弃。
    pub(crate) async fn current_books(&self, max: usize, active_days: i64) -> Result<Vec<ReadingBook>> {
        let shelf = self.call("/shelf/sync", json!({})).await?;
        let books = shelf
            .get("books")
            .and_then(Value::as_array)
            .ok_or_else(|| Error::BadResponse("书架响应缺少 books 数组".into()))?;
        let cutoff = Local::now() - Duration::days(active_days);
        let mut candidates: Vec<(String, String, String, DateTime<Local>)> = Vec::new();
        for b in books {
            let read_at = b
                .get("readUpdateTime")
                .and_then(Value::as_i64)
                .and_then(|ts| local_datetime(ts));
            let Some(read_at) = read_at else { continue };
            let finished = b.get("finishReading").and_then(Value::as_i64) == Some(1);
            if finished || read_at < cutoff {
                continue;
            }
            let title = b.get("title").and_then(Value::as_str).unwrap_or_default().to_string();
            if title.is_empty() {
                continue;
            }
            candidates.push((
                b.get("bookId").and_then(Value::as_str).unwrap_or_default().to_string(),
                title,
                b.get("author").and_then(Value::as_str).unwrap_or_default().to_string(),
                read_at,
            ));
        }
        candidates.sort_by(|a, b| b.3.cmp(&a.3));
        let mut result = Vec::new();
        for (book_id, title, author, last_read) in candidates.into_iter().take(max) {
            let payload = match self.call("/book/getprogress", json!({ "bookId": book_id })).await {
                Ok(p) => p,
                // 单本进度失败不拖垮整块仪表盘，跳过即可。
                Err(Error::BadResponse(_)) => continue,
                Err(e) => return Err(e),
            };
            match parse_progress(&payload) {
                // 网关语义：1 = 1%，只有 100 才是读完；读完但书架未标记的书不展示。
                Ok(p) if p == 100 => {}
                Ok(progress) => result.push(ReadingBook {
                    title,
                    author,
                    progress_percent: progress,
                    record_seconds: parse_record_seconds(&payload),
                    last_read: Some(last_read),
                }),
                Err(_) => continue,
            }
        }
        Ok(result)
    }
}

/// 回包信封：upgrade_info 优先拦截；errcode 非 0 报业务错误；
/// 业务字段兼容顶层平铺与 `data` 嵌套两种形态（官方文档未固定，实测以真机为准）。
fn envelope(response: Value) -> Result<Value> {
    if let Some(upgrade) = response.get("upgrade_info") {
        let message = upgrade
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("请升级微信读书 Skill");
        return Err(Error::UpgradeRequired(message.into()));
    }
    let errcode = response.get("errcode").and_then(Value::as_i64).unwrap_or(0);
    if errcode != 0 {
        let message = response
            .get("errmsg")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| format!("errcode={errcode}"));
        return Err(Error::Gateway(message));
    }
    match response.get("data") {
        Some(Value::Object(fields)) => Ok(Value::Object(fields.clone())),
        _ => Ok(response),
    }
}

fn local_datetime(ts: i64) -> Option<DateTime<Local>> {
    Local.timestamp_opt(ts, 0).single()
}

fn u64_of(value: &Value) -> Option<u64> {
    value.as_f64().filter(|n| *n >= 0.0 && n.fract() == 0.0).map(|n| n as u64)
}

/// weekly 响应 → 周一..周日的 7 桶；readTimes 的 key 是分桶起始时间戳，
/// 按本地日期对齐到本周（周一为下标 0），落在本周之外的桶忽略。
fn parse_weekly(payload: &Value) -> Result<WeekStats> {
    let bad = |what: &str| Error::BadResponse(format!("阅读统计响应无效：{what}"));
    let total_seconds = payload
        .get("totalReadTime")
        .and_then(u64_of)
        .ok_or_else(|| bad("缺少 totalReadTime（秒）"))?;
    let read_days = payload
        .get("readDays")
        .and_then(Value::as_u64)
        .ok_or_else(|| bad("缺少 readDays"))? as u32;
    let compare = match payload.get("compare") {
        None | Some(Value::Null) => None,
        Some(v) => Some(v.as_f64().ok_or_else(|| bad("compare 非数值"))? as f32),
    };
    let today = Local::now().date_naive();
    let monday = today - Duration::days(today.weekday().num_days_from_monday() as i64);
    let mut seconds = [0u64; 7];
    if let Some(buckets) = payload.get("readTimes").and_then(Value::as_object) {
        for (key, value) in buckets {
            // 网关可能附加非时间戳键（如版本标记）；无法解析的键跳过，不拖垮整块统计。
            let Ok(ts) = key.parse::<i64>() else {
                continue;
            };
            let Some(date) = local_datetime(ts).map(|d| d.date_naive()) else {
                continue;
            };
            let offset = (date - monday).num_days();
            if (0..7).contains(&offset) {
                let seconds_value = u64_of(value).ok_or_else(|| bad("readTimes 值非整数秒"))?;
                seconds[offset as usize] = seconds_value;
            }
        }
    }
    let days = (0..7)
        .map(|i| DayStat { date: monday + Duration::days(i), seconds: seconds[i as usize] })
        .collect();
    Ok(WeekStats { days, total_seconds, read_days, compare })
}

/// progress 响应校验：必须是 0..=100 整数（网关语义 1 = 1%）。
fn parse_progress(payload: &Value) -> Result<u8> {
    let bad = || Error::BadResponse("进度响应无效：progress 应为 0..=100 整数".into());
    let progress = payload
        .get("book")
        .and_then(|b| b.get("progress"))
        .and_then(Value::as_i64)
        .ok_or_else(bad)?;
    if !(0..=100).contains(&progress) {
        return Err(bad());
    }
    Ok(progress as u8)
}

fn parse_record_seconds(payload: &Value) -> u64 {
    payload
        .get("book")
        .and_then(|b| b.get("recordReadingTime"))
        .and_then(u64_of)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn weekly_body(monday_ts: i64) -> Value {
        json!({
            "errcode": 0,
            "baseTime": monday_ts,
            "readTimes": {
                (monday_ts.to_string()): 1860,
                ((monday_ts + 86400).to_string()): 2700.0,
                "not-a-timestamp": 100
            },
            "totalReadTime": 4560,
            "readDays": 2,
            "compare": 0.2
        })
    }

    #[test]
    fn weekly_fills_monday_first_and_zeroes_future_days() {
        let today = Local::now().date_naive();
        let monday = today - Duration::days(today.weekday().num_days_from_monday() as i64);
        let monday_ts = monday.and_hms_opt(0, 0, 0).unwrap().and_local_timezone(Local).unwrap().timestamp();
        let stats = parse_weekly(&envelope(weekly_body(monday_ts)).unwrap()).unwrap();
        assert_eq!(stats.days.len(), 7);
        assert_eq!(stats.days[0].date, monday);
        assert_eq!(stats.days[0].seconds, 1860);
        assert_eq!(stats.days[1].seconds, 2700);
        assert!(stats.days[2..].iter().all(|d| d.seconds == 0));
        assert_eq!(stats.total_seconds, 4560);
        assert_eq!(stats.read_days, 2);
        assert_eq!(stats.compare, Some(0.2));
    }

    #[test]
    fn weekly_ignores_buckets_outside_this_week() {
        let stale_ts = 1_600_000_000; // 2020-09，远早于本周
        let mut body = weekly_body(stale_ts);
        body["readTimes"] = json!({ stale_ts.to_string(): 9999 });
        let stats = parse_weekly(&envelope(body).unwrap()).unwrap();
        assert!(stats.days.iter().all(|d| d.seconds == 0));
        assert_eq!(stats.total_seconds, 4560);
    }

    #[test]
    fn envelope_routes_upgrade_errcode_and_data_shapes() {
        let upgrade = envelope(json!({"upgrade_info": {"message": "请升级到 1.1.0"}}));
        assert!(matches!(upgrade, Err(Error::UpgradeRequired(m)) if m.contains("1.1.0")));
        let err = envelope(json!({"errcode": 40001, "errmsg": "apikey 无效"}));
        assert!(matches!(err, Err(Error::Gateway(m)) if m.contains("apikey")));
        let nested = envelope(json!({"errcode": 0, "data": {"totalReadTime": 60}})).unwrap();
        assert_eq!(nested["totalReadTime"], json!(60));
        let flat = envelope(json!({"errcode": 0, "totalReadTime": 60})).unwrap();
        assert_eq!(flat["totalReadTime"], json!(60));
    }

    #[test]
    fn progress_bounds_are_strict() {
        let make = |p: Value| parse_progress(&json!({"book": {"progress": p}}));
        assert_eq!(make(json!(0)).unwrap(), 0);
        assert_eq!(make(json!(1)).unwrap(), 1);
        assert_eq!(make(json!(100)).unwrap(), 100);
        assert!(make(json!(101)).is_err());
        assert!(make(json!(-1)).is_err());
        assert!(make(json!(45.5)).is_err());
        assert!(parse_progress(&json!({})).is_err());
    }

    #[test]
    fn record_seconds_defaults_to_zero_when_absent() {
        assert_eq!(parse_record_seconds(&json!({"book": {"progress": 50}})), 0);
        assert_eq!(
            parse_record_seconds(&json!({"book": {"progress": 50, "recordReadingTime": 43200}})),
            43200
        );
    }
}
