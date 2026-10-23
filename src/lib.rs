//! 微信读书墨水屏仪表盘 SDK：微信读书网关取数 → 渲染 400×300 PNG → Zectrix 设备推屏。
//!
//! 入口：[`fetch_dashboard`]（取数）、[`render_dashboard`](渲染)、[`push`]（推屏），
//! 或用 [`run_once`] 串起全流程。CLI 见 `weread-ink` 二进制。
mod font;
mod push;
mod render;
mod weread;

pub use push::{push, PushOptions};
pub use zectrix_sdk::ImageFormat;
use zectrix_sdk::Event;

use chrono::{DateTime, Local, NaiveDate};
use std::path::PathBuf;

/// SDK 所有操作返回业务错误，不暴露 HTTP / 协议细节。
pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("缺少 WEREAD_API_KEY（wrk- 开头），请到微信读书 Agent API Gateway 申请")]
    NoApiKey,
    #[error("微信读书网关错误：{0}")]
    Gateway(String),
    #[error("微信读书 Skill 需要升级：{0}")]
    UpgradeRequired(String),
    #[error("网关响应无效：{0}")]
    BadResponse(String),
    #[error("网络请求失败：{0}")]
    Http(String),
    #[error("找不到可用中文字体，请用 WEREAD_FONT 指向 ttf/otf/ttc 文件")]
    NoFont,
    #[error("渲染失败：{0}")]
    Render(String),
    #[error("设备推送失败：{0}")]
    Device(#[from] zectrix_sdk::Error),
}
impl From<reqwest::Error> for Error {
    fn from(e: reqwest::Error) -> Self {
        Self::Http(e.to_string())
    }
}

/// 取数配置。`base_url` 默认官方网关，测试时指向本地假网关。
#[derive(Clone, Debug, Default)]
pub struct Config {
    pub api_key: String,
    pub base_url: Option<String>,
    pub font_path: Option<PathBuf>,
}
impl Config {
    pub fn from_env() -> Result<Self> {
        let api_key = std::env::var("WEREAD_API_KEY").unwrap_or_default();
        if api_key.trim().is_empty() {
            return Err(Error::NoApiKey);
        }
        Ok(Self {
            api_key,
            base_url: std::env::var("WEREAD_BASE_URL").ok().filter(|s| !s.is_empty()),
            font_path: std::env::var("WEREAD_FONT").ok().map(PathBuf::from),
        })
    }
}

/// 本周（周一至周日）每日阅读时长，单位秒。
#[derive(Clone, Debug, PartialEq)]
pub struct WeekStats {
    /// 固定 7 项，下标 0 = 周一；未到/未读的天为 0。
    pub days: Vec<DayStat>,
    /// 本周总时长（秒），来自网关 totalReadTime 口径。
    pub total_seconds: u64,
    /// 有效阅读天数（单日满 1 分钟计 1 天）。
    pub read_days: u32,
    /// 与上周日均对比，0.2 表示约 +20%；无对比数据时为 None。
    pub compare: Option<f32>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DayStat {
    pub date: NaiveDate,
    pub seconds: u64,
}

/// 一本最近在读的书。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReadingBook {
    pub title: String,
    pub author: String,
    /// 阅读进度百分比 0~100；网关返回越界值视为响应无效。
    pub progress_percent: u8,
    /// 本书累计阅读时长（秒）。
    pub record_seconds: u64,
    /// 最后阅读时间；缺失时为 None。
    pub last_read: Option<DateTime<Local>>,
}

/// 一次取数得到的完整仪表盘数据。
#[derive(Clone, Debug)]
pub struct Dashboard {
    pub week: WeekStats,
    pub books: Vec<ReadingBook>,
    pub generated_at: DateTime<Local>,
}
impl Dashboard {
    pub fn today_seconds(&self) -> u64 {
        let today = Local::now().date_naive();
        self.week
            .days
            .iter()
            .find(|d| d.date == today)
            .map(|d| d.seconds)
            .unwrap_or(0)
    }
    /// CLI / 日志用的单行摘要。
    pub fn summary_text(&self) -> String {
        let mut s = format!(
            "今日 {} · 本周 {} · 阅读 {} 天",
            format_duration(self.today_seconds()),
            format_duration(self.week.total_seconds),
            self.week.read_days
        );
        if let Some(c) = self.week.compare {
            s.push_str(&format!(" · 较上周 {}{:.0}%", if c >= 0.0 { "+" } else { "" }, c * 100.0));
        }
        for b in &self.books {
            s.push_str(&format!(
                "\n《{}》{} · {}% · 累计 {}",
                b.title,
                b.author,
                b.progress_percent,
                format_duration(b.record_seconds)
            ));
        }
        s
    }
}

/// 秒 → "X小时Y分钟" / "Y分钟" / "不足1分钟"。
pub fn format_duration(seconds: u64) -> String {
    let h = seconds / 3600;
    let m = (seconds % 3600) / 60;
    match (h, m) {
        (0, 0) => "不足1分钟".into(),
        (0, m) => format!("{m}分钟"),
        (h, 0) => format!("{h}小时"),
        (h, m) => format!("{h}小时{m}分"),
    }
}

/// 拉取本周统计 + 最近在读（最多 2 本，取最近 14 天内读过且未读完的书）。
pub async fn fetch_dashboard(config: &Config) -> Result<Dashboard> {
    let gateway = weread::Gateway::new(config)?;
    let week = gateway.weekly().await?;
    let books = gateway.current_books(2, 14).await?;
    Ok(Dashboard {
        week,
        books,
        generated_at: Local::now(),
    })
}

/// 渲染 400×300 灰度 PNG。`font_bytes` 来自 [`font::load_font`]。
pub fn render_dashboard(dashboard: &Dashboard, font_bytes: &[u8]) -> Result<Vec<u8>> {
    render::render(dashboard, font_bytes)
}

/// 加载中文字体：优先 `preferred`，其次 fonts/ 目录与系统字体。
pub fn load_font(preferred: Option<&std::path::Path>) -> Result<Vec<u8>> {
    font::load(preferred)
}

/// 取数 + 渲染 + 推屏一条龙。`dry_run` 提供“Yes 路径”时只落盘 PNG 不碰设备。
pub async fn run_once(
    config: &Config,
    push_options: &PushOptions,
    dry_run: Option<&std::path::Path>,
) -> Result<Dashboard> {
    let dashboard = fetch_dashboard(config).await?;
    let font = font::load(config.font_path.as_deref())?;
    let png = render::render(&dashboard, &font)?;
    if let Some(path) = dry_run {
        std::fs::write(path, &png).map_err(|e| Error::Render(e.to_string()))?;
    } else {
        push(&png, push_options).await?;
    }
    Ok(dashboard)
}

/// 长驻模式：设备连接只建立一次并保持，每 interval 秒刷新一帧。
/// 该固件在管理连接关闭后会恢复原画面，常显必须维持会话；
/// 会话异常结束后 30 秒自动重连。适配 launchd KeepAlive 常驻进程。
/// 全部日志走 stderr（无缓冲，launchd 落盘实时可见）。
pub async fn run_daemon(config: &Config, push_options: &PushOptions, interval_secs: u64) -> ! {
    let mut last_png: Option<Vec<u8>> = None;
    loop {
        if let Err(e) = daemon_session(config, push_options, interval_secs, &mut last_png).await {
            dlog(&format!("会话结束：{e}，10 秒后重连"));
        }
        tokio::time::sleep(std::time::Duration::from_secs(10)).await;
    }
}

fn dlog(message: &str) {
    eprintln!("[daemon][{}] {message}", chrono::Local::now().format("%m-%d %H:%M:%S"));
}

async fn daemon_session(
    config: &Config,
    push_options: &PushOptions,
    interval_secs: u64,
    last_png: &mut Option<Vec<u8>>,
) -> Result<()> {
    let device = std::sync::Arc::new(push::open_device(push_options).await?);
    dlog(&format!(
        "设备连接已建立：{}（{}）",
        device.summary.address, device.summary.name
    ));
    // 事件排水：满足 SDK 单消费者契约；任何断开立即发信号中断主循环的定时长等待，
    // Mac 睡眠唤醒后 ~15 秒内（心跳超时判定）就能重连补帧，而不是等满下一个周期。
    let (drop_tx, mut drop_rx) = tokio::sync::watch::channel(());
    let drain_device = device.clone();
    tokio::spawn(async move {
        loop {
            let ended = match drain_device.next_event().await {
                Ok(Some(Event::Disconnected { reason })) => {
                    dlog(&format!("设备事件：断开（{reason:?}）"));
                    true
                }
                Ok(Some(event)) => {
                    dlog(&format!("设备事件：{event:?}"));
                    false
                }
                Ok(None) => {
                    dlog("设备事件：流结束");
                    true
                }
                Err(_) => true,
            };
            if ended {
                let _ = drop_tx.send(());
                break;
            }
        }
    });
    // 重连后先推上一帧缓存，屏幕尽快恢复，不必等网关取数。
    if let Some(png) = last_png.clone() {
        match push::show(&device, &png, push_options.format).await {
            Ok(()) => dlog("缓存帧已恢复"),
            Err(e) => return Err(e),
        }
    }
    loop {
        // 取数/渲染失败只跳过本轮，不断开设备会话。
        match fetch_dashboard(config).await {
            Ok(dashboard) => {
                let font = font::load(config.font_path.as_deref())?;
                let png = render::render(&dashboard, &font)?;
                push::show(&device, &png, push_options.format).await?;
                *last_png = Some(png);
                dlog(&format!(
                    "已刷新：{}",
                    dashboard.summary_text().replace('\n', " | ")
                ));
            }
            Err(e) => dlog(&format!("取数失败，本轮跳过：{e}")),
        }
        tokio::select! {
            _ = tokio::time::sleep(std::time::Duration::from_secs(interval_secs)) => {}
            _ = drop_rx.changed() => {
                return Err(Error::Device(zectrix_sdk::Error::Disconnected));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn duration_formats_cover_hours_minutes_and_floor() {
        assert_eq!(format_duration(0), "不足1分钟");
        assert_eq!(format_duration(59), "不足1分钟");
        assert_eq!(format_duration(60), "1分钟");
        assert_eq!(format_duration(2700), "45分钟");
        assert_eq!(format_duration(3600), "1小时");
        assert_eq!(format_duration(15600), "4小时20分");
    }
    #[test]
    fn today_seconds_finds_local_date_bucket() {
        let mut days: Vec<DayStat> = (0..7)
            .map(|i| DayStat {
                date: Local::now().date_naive() - chrono::Duration::days(i as i64),
                seconds: 0,
            })
            .collect();
        days[1].seconds = 1800;
        let dashboard = Dashboard {
            week: WeekStats { days, total_seconds: 1800, read_days: 1, compare: None },
            books: vec![],
            generated_at: Local::now(),
        };
        assert_eq!(dashboard.today_seconds(), 0);
    }
}
