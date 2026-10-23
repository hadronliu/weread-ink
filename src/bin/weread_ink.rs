//! weread-ink CLI：取数 → 渲染 → 推屏（或 --dry-run 落盘 PNG）。
use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "用法：weread-ink [选项]
  --daemon            长驻模式：保持设备连接，每 interval 秒刷新（推荐配 launchd）
  --interval <秒>     daemon 刷新间隔，默认 1800（半小时）
  --target <IP|ID>    设备目标（默认 WEREAD_DEVICE_TARGET，再默认自动发现）
  --format <F>        mono | gray4 | gray16（默认 gray4）
  --font <PATH>       中文字体路径（默认 WEREAD_FONT，再默认系统探测）
  --base-url <URL>    覆盖网关地址（调试用）
  --dry-run <FILE>    只渲染并保存 PNG，不连接设备
  --help              显示帮助

环境变量：WEREAD_API_KEY 必填（wrk- 开头）；WEREAD_INTERVAL 可设刷新间隔";

fn main() -> ExitCode {
    load_dotenv();
    let mut target = std::env::var("WEREAD_DEVICE_TARGET").ok().filter(|s| !s.is_empty());
    let mut font: Option<PathBuf> = std::env::var("WEREAD_FONT").ok().map(PathBuf::from);
    let mut base_url = std::env::var("WEREAD_BASE_URL").ok().filter(|s| !s.is_empty());
    let mut dry_run: Option<PathBuf> = None;
    let mut format = weread_ink::ImageFormat::Gray4;
    let mut daemon = false;
    let mut interval: u64 = std::env::var("WEREAD_INTERVAL")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1800);
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = || {
            args.next().unwrap_or_else(|| {
                eprintln!("缺少 {arg} 的值\n{USAGE}");
                std::process::exit(2);
            })
        };
        match arg.as_str() {
            "--daemon" => daemon = true,
            "--interval" => {
                interval = value().parse().unwrap_or_else(|_| {
                    eprintln!("--interval 应为秒数\n{USAGE}");
                    std::process::exit(2);
                })
            }
            "--target" => target = Some(value()),
            "--font" => font = Some(PathBuf::from(value())),
            "--base-url" => base_url = Some(value()),
            "--dry-run" => dry_run = Some(PathBuf::from(value())),
            "--format" => match value().as_str() {
                "mono" => format = weread_ink::ImageFormat::Mono,
                "gray4" => format = weread_ink::ImageFormat::Gray4,
                "gray16" => format = weread_ink::ImageFormat::Gray16,
                _ => return exit_usage("format 只能是 mono、gray4 或 gray16"),
            },
            "--help" | "-h" => {
                println!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            _ => return exit_usage(&format!("未知参数：{arg}")),
        }
    }
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("无法启动 tokio 运行时");
    let config = weread_ink::Config {
        api_key: std::env::var("WEREAD_API_KEY").unwrap_or_default(),
        base_url,
        font_path: font,
    };
    let push_options = weread_ink::PushOptions { target, format, ..Default::default() };
    if daemon {
        if dry_run.is_some() {
            return exit_usage("--daemon 与 --dry-run 不能同时使用");
        }
        println!(
            "[daemon] 启动：每 {} 秒刷新（{}）",
            interval,
            push_options.target.as_deref().unwrap_or("自动发现设备")
        );
        runtime.block_on(weread_ink::run_daemon(&config, &push_options, interval));
    }
    match runtime.block_on(weread_ink::run_once(&config, &push_options, dry_run.as_deref())) {
        Ok(dashboard) => {
            println!("{}", dashboard.summary_text());
            if let Some(path) = &dry_run {
                println!("已渲染：{}", path.display());
            } else {
                println!("已推送到设备");
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("失败：{e}");
            ExitCode::FAILURE
        }
    }
}

/// 极简 .env 加载：只认 KEY=VALUE 行，不覆盖已存在的环境变量。密钥不进代码库。
fn load_dotenv() {
    let Ok(content) = std::fs::read_to_string(".env") else { return };
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            let value = value.trim().trim_matches('"');
            if std::env::var(key).is_err() {
                std::env::set_var(key, value);
            }
        }
    }
}

fn exit_usage(message: &str) -> ExitCode {
    eprintln!("{message}\n{USAGE}");
    ExitCode::FAILURE
}
