//! 推屏：连接 Zectrix 设备 → 显示 PNG → 释放连接。墨水屏断电保持画面，推完即走。
use crate::{Error, ImageFormat, Result};
use zectrix_sdk::{Device, OpenOptions};

/// 推屏选项。`format` 建议 Gray4：进度条和柱状图有灰阶层次。
#[derive(Clone, Debug)]
pub struct PushOptions {
    /// IPv4 / IPv4:端口 / 12 位设备 ID；None 时自动发现唯一设备。
    pub target: Option<String>,
    pub client_name: Option<String>,
    pub format: ImageFormat,
}
impl Default for PushOptions {
    fn default() -> Self {
        Self {
            target: None,
            client_name: Some("Weread Ink".into()),
            format: ImageFormat::Gray4,
        }
    }
}
impl PushOptions {
    pub fn from_env() -> Self {
        Self {
            target: std::env::var("WEREAD_DEVICE_TARGET").ok().filter(|s| !s.is_empty()),
            ..Default::default()
        }
    }
}

/// 显示一张 PNG，等待设备报告物理刷新完成后返回。
/// 实测怪癖（zectrix-s3-epaper-4.2 固件）：显示提交后立刻断开会取消物理刷新，
/// 协议仍应答“完成”但屏幕不动。因此成功后保持心跳数秒，让刷新波形真正落地。
/// 设备 WiFi 深度省电时 2 秒 INFO 探测可能错过窗口，对 NoDevice/传输失败做短退避重试。
pub async fn push(png: &[u8], options: &PushOptions) -> Result<()> {
    let mut last_err = None;
    for attempt in 0..3 {
        if attempt > 0 {
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        }
        match push_once(png, options).await {
            Ok(()) => return Ok(()),
            Err(e @ Error::Device(zectrix_sdk::Error::NoDevice))
            | Err(e @ Error::Device(zectrix_sdk::Error::TransportFailure(_))) => {
                last_err = Some(e);
                continue;
            }
            Err(e) => return Err(e),
        }
    }
    Err(last_err.expect("至少一次重试才可能到这里"))
}

async fn push_once(png: &[u8], options: &PushOptions) -> Result<()> {
    let device = open_device(options).await?;
    let shown = show(&device, png, options.format).await;
    if shown.is_ok() {
        tokio::time::sleep(std::time::Duration::from_secs(8)).await;
    }
    let _ = device.close().await;
    shown
}

/// 打开设备连接（daemon 模式复用同一连接反复推送）。
pub(crate) async fn open_device(options: &PushOptions) -> Result<Device> {
    Device::open(OpenOptions {
        target: options.target.clone(),
        client_name: options.client_name.clone(),
    })
    .await
    .map_err(Error::from)
}

/// 在已有连接上推送一张图（不负责连接生命周期）。
pub(crate) async fn show(device: &Device, png: &[u8], format: ImageFormat) -> Result<()> {
    device.show_with_format(png, format).await.map_err(Error::from)
}
