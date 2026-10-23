//! 假网关集成测试：在 loopback HTTP 上模拟微信读书 Agent API Gateway，
//! 用真实 reqwest 客户端走完整取数流程（沿用 zectrix "模拟设备先行" 的测试思路）。
use chrono::{Datelike, Local};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use weread_ink::{Config, Error, ImageFormat};

type Recorded = Arc<Mutex<Vec<(String, String, Value)>>>; // (api, auth, body)

/// 可注入行为的假网关：`mode` 切换正常 / 需要升级 / 鉴权失败三种剧本。
struct FakeGateway {
    url: String,
    requests: Recorded,
}

impl FakeGateway {
    async fn start(mode: &'static str) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let requests: Recorded = Arc::new(Mutex::new(Vec::new()));
        let sink = requests.clone();
        tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else { break };
                let sink = sink.clone();
                tokio::spawn(async move {
                    let mut buffer = [0u8; 8192];
                    let mut raw = Vec::new();
                    // 读到请求头结束，再按 Content-Length 补齐 body。
                    loop {
                        let n = match stream.read(&mut buffer).await {
                            Ok(0) | Err(_) => return,
                            Ok(n) => n,
                        };
                        raw.extend_from_slice(&buffer[..n]);
                        if let Some(h) = find_header_end(&raw) {
                            let length = content_length(&raw[..h]);
                            if raw.len() >= h + length {
                                break;
                            }
                        }
                    }
                    let split = find_header_end(&raw).expect("请求头不完整");
                    let header = String::from_utf8_lossy(&raw[..split]).to_string();
                    let length = content_length(&raw[..split]);
                    let body: Value = serde_json::from_slice(&raw[split..split + length]).unwrap();
                    let auth = header
                        .lines()
                        .find_map(|l| l.split_once(':'))
                        .filter(|(k, _)| k.eq_ignore_ascii_case("authorization"))
                        .map(|(_, v)| v.trim().to_string())
                        .unwrap_or_default();
                    let api = body["api_name"].as_str().unwrap_or_default().to_string();
                    sink.lock().unwrap().push((api.clone(), auth, body.clone()));
                    let response = route(&api, &body, mode);
                    serve(&mut stream, &response).await;
                });
            }
        });
        Self { url, requests }
    }

    fn take_requests(&self) -> Vec<(String, String, Value)> {
        std::mem::take(&mut *self.requests.lock().unwrap())
    }
}

async fn serve(stream: &mut tokio::net::TcpStream, response: &Value) {
    let payload = serde_json::to_vec(response).unwrap();
    let header = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        payload.len()
    );
    let _ = stream.write_all(header.as_bytes()).await;
    let _ = stream.write_all(&payload).await;
}

fn find_header_end(raw: &[u8]) -> Option<usize> {
    raw.windows(4).position(|w| w == b"\r\n\r\n").map(|p| p + 4)
}

fn content_length(header: &[u8]) -> usize {
    String::from_utf8_lossy(header)
        .lines()
        .filter_map(|l| l.split_once(':'))
        .find(|(k, _)| k.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, v)| v.trim().parse().ok())
        .unwrap_or(0)
}

fn route(api: &str, body: &Value, mode: &str) -> Value {
    if mode == "upgrade" {
        return json!({"upgrade_info": {"message": "请升级 Skill 到 1.1.0"}});
    }
    if mode == "auth" {
        return json!({"errcode": 40001, "errmsg": "apikey 无效"});
    }
    match api {
        "/readdata/detail" => {
            let today = Local::now().date_naive();
            let monday = today - chrono::Duration::days(today.weekday().num_days_from_monday() as i64);
            let ts = |d: i64| {
                (monday + chrono::Duration::days(d))
                    .and_hms_opt(0, 0, 0)
                    .unwrap()
                    .and_local_timezone(Local)
                    .unwrap()
                    .timestamp()
            };
            json!({
                "errcode": 0,
                "readTimes": { ts(0).to_string(): 1860, ts(1).to_string(): 2700 },
                "totalReadTime": 4560,
                "readDays": 2,
                "compare": 0.2
            })
        }
        "/shelf/sync" => {
            let now = Local::now().timestamp();
            json!({
                "errcode": 0,
                "books": [
                    {"bookId": "B1", "title": "三体", "author": "刘慈欣", "readUpdateTime": now - 7200, "finishReading": 0},
                    {"bookId": "B2", "title": "活着", "author": "余华", "readUpdateTime": now - 86400, "finishReading": 0},
                    {"bookId": "B3", "title": "旧书", "author": "某某", "readUpdateTime": now - 30*86400, "finishReading": 0},
                    {"bookId": "B4", "title": "读完的书", "author": "某某", "readUpdateTime": now - 3600, "finishReading": 1}
                ]
            })
        }
        "/book/getprogress" => match body["bookId"].as_str() {
            Some("B1") => json!({"errcode": 0, "book": {"progress": 62, "recordReadingTime": 43200, "updateTime": Local::now().timestamp() - 7200, "isStartReading": 1}}),
            Some("B2") => json!({"errcode": 0, "book": {"progress": 100, "recordReadingTime": 3600, "finishTime": Local::now().timestamp()}}),
            _ => json!({"errcode": 40404, "errmsg": "bookId 不存在"}),
        },
        _ => json!({"errcode": 40400, "errmsg": "未知接口"}),
    }
}

fn config_for(gateway: &FakeGateway) -> Config {
    Config {
        api_key: "wrk-test-key".into(),
        base_url: Some(gateway.url.clone()),
        font_path: None,
    }
}

#[tokio::test]
async fn happy_path_builds_dashboard_and_reports_skill_version() {
    let gateway = FakeGateway::start("ok").await;
    let dashboard = weread_ink::fetch_dashboard(&config_for(&gateway)).await.unwrap();
    let today = Local::now().date_naive();
    assert_eq!(dashboard.week.days.len(), 7);
    assert_eq!(dashboard.week.days[0].seconds, 1860);
    assert_eq!(dashboard.week.days[1].seconds, 2700);
    assert_eq!(dashboard.week.total_seconds, 4560);
    assert_eq!(dashboard.week.read_days, 2);
    // B4 已读完、B3 超过 14 天、B2 进度 100%，只剩 B1。
    assert_eq!(dashboard.books.len(), 1);
    assert_eq!(dashboard.books[0].title, "三体");
    assert_eq!(dashboard.books[0].progress_percent, 62);
    assert_eq!(dashboard.books[0].record_seconds, 43200);
    let idx = today.weekday().num_days_from_monday() as usize;
    let expected = [1860u64, 2700, 0, 0, 0, 0, 0][idx];
    assert_eq!(dashboard.today_seconds(), expected);
    let requests = gateway.take_requests();
    assert!(!requests.is_empty());
    assert!(requests.iter().all(|(_, auth, _)| auth == "Bearer wrk-test-key"));
    assert!(requests.iter().all(|(_, _, body)| body["skill_version"] == json!("1.0.5")));
    let apis: Vec<&str> = requests.iter().map(|(api, _, _)| api.as_str()).collect();
    assert!(apis.contains(&"/readdata/detail"));
    assert!(apis.contains(&"/shelf/sync"));
    assert!(apis.contains(&"/book/getprogress"));
}

#[tokio::test]
async fn upgrade_info_blocks_everything() {
    let gateway = FakeGateway::start("upgrade").await;
    let err = weread_ink::fetch_dashboard(&config_for(&gateway)).await.unwrap_err();
    assert!(matches!(err, Error::UpgradeRequired(m) if m.contains("1.1.0")));
}

#[tokio::test]
async fn gateway_error_surfaces_business_message() {
    let gateway = FakeGateway::start("auth").await;
    let err = weread_ink::fetch_dashboard(&config_for(&gateway)).await.unwrap_err();
    assert!(matches!(err, Error::Gateway(m) if m.contains("apikey")));
}

#[tokio::test]
async fn missing_api_key_is_rejected_before_any_request() {
    let err = weread_ink::fetch_dashboard(&Config::default()).await.unwrap_err();
    assert!(matches!(err, Error::NoApiKey));
}

/// 渲染 + 假网关联动：确认 run_once 的 dry-run 路径能产出合法 PNG。
#[tokio::test]
async fn dry_run_renders_png_without_device() {
    let gateway = FakeGateway::start("ok").await;
    let dir = std::env::temp_dir().join(format!("weread-ink-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let png_path = dir.join("dash.png");
    let options = weread_ink::PushOptions { target: None, client_name: None, format: ImageFormat::Mono };
    weread_ink::run_once(&config_for(&gateway), &options, Some(&png_path)).await.unwrap();
    let png = std::fs::read(&png_path).unwrap();
    let img = image::load_from_memory(&png).unwrap();
    assert_eq!((img.width(), img.height()), (400, 300));
    let _ = std::fs::remove_dir_all(&dir);
}
