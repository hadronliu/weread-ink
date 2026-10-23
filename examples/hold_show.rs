//! 对照实验：长连接模式推屏，模仿 agent-pet-local 的会话形态（连接后保持心跳，多次推送）。
//! 用法：cargo run --example hold_show -- [target] image1.png image2.png
use std::time::Duration;
use zectrix_sdk::{Device, ImageFormat, OpenOptions};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let target = args.get(1).cloned();
    let first = std::fs::read(args.get(2).map(String::as_str).unwrap_or("sample.png"))?;
    let second = args.get(3).map(|p| std::fs::read(p)).transpose()?;
    let device = Device::open(OpenOptions {
        target,
        client_name: Some("hold-show-test".into()),
    })
    .await?;
    println!("[{}] 已连接，推送第 1 张…", timestamp());
    device.show_with_format(&first, ImageFormat::Gray4).await?;
    println!("[{}] 第 1 张显示完成，保持连接 25 秒（心跳运行）…", timestamp());
    tokio::time::sleep(Duration::from_secs(25)).await;
    if let Some(second) = second {
        println!("[{}] 推送第 2 张…", timestamp());
        device.show_with_format(&second, ImageFormat::Gray4).await?;
        println!("[{}] 第 2 张显示完成，保持连接 10 分钟（期间观察是否被恢复原界面）…", timestamp());
        tokio::time::sleep(Duration::from_secs(600)).await;
    }
    device.close().await?;
    println!("[{}] 已关闭连接", timestamp());
    Ok(())
}

fn timestamp() -> String {
    chrono::Local::now().format("%H:%M:%S").to_string()
}
