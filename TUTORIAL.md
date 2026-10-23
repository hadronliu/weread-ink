# weread-ink 使用教程

从零开始，把你的微信读书阅读数据放上墨水屏。全程约 30 分钟（不含固件核对）。

效果图：

![面板效果](docs/sample.png)

---

## 目录

1. [你需要准备什么](#1-你需要准备什么)
2. [获取项目和设备 SDK](#2-获取项目和设备-sdk)
3. [编译与测试](#3-编译与测试)
4. [预览渲染效果（不需要任何硬件）](#4-预览渲染效果)
5. [配置 API Key 和设备](#5-配置-api-key-和设备)
6. [第一次推屏](#6-第一次推屏)
7. [定时常驻（推荐）](#7-定时常驻推荐)
8. [日常操作速查](#8-日常操作速查)
9. [排障手册](#9-排障手册)
10. [个性化](#10-个性化)
11. [与其他共屏程序的关系](#11-与其他共屏程序的关系)
12. [常见问题](#12-常见问题)

---

## 1. 你需要准备什么

| 项 | 要求 | 说明 |
| --- | --- | --- |
| 电脑 | macOS（Apple Silicon 已验证）/ Linux / Windows | 需与设备同一局域网 |
| Rust | stable 工具链 | `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \| sh` |
| CMake | opus 依赖编译需要 | macOS: `brew install cmake` |
| 墨水屏设备 | Zectrix Note 4（`zectrix-s3-epaper-4.2`，固件 3.7.2） | 不适用于 Note 4C |
| 微信读书 API Key | `wrk-` 开头 | 见下方申请方式 |
| 中文字体 | 无需准备 | 项目内置开源点阵字体 |

**申请微信读书 API Key**：这是腾讯官方提供的 Agent API 网关凭据，
到官方页面 <https://weread.qq.com/r/weread-skills> 按指引开通，获得 `wrk-` 开头的 Key。
数据走官方接口，不需要抓包或逆向。

**查设备固件版本**：设备「设置 → 关于」核对为 3.7.2；同时建议在
「设置 → 网络同步」开启「待机保持 Wi-Fi」，可显著减少推送时的唤醒失败。

## 2. 获取项目和设备 SDK

本项目的设备通信依赖 [zectrix-local-push-sdk](https://github.com/itopinion/zectrix-local-push-sdk)，
默认按**同级目录路径依赖**引用。把两个仓库克隆到同一父目录：

```bash
mkdir ~/ink-projects && cd ~/ink-projects
git clone https://github.com/itopinion/zectrix-local-push-sdk.git
git clone https://github.com/<你的用户名>/weread-ink.git   # 或你自己的 fork
```

得到的目录结构：

```
~/ink-projects/
├── zectrix-local-push-sdk/
└── weread-ink/
```

> 不想按目录放置？把 `weread-ink/Cargo.toml` 里的依赖改成
> `zectrix-sdk = { git = "https://github.com/itopinion/zectrix-local-push-sdk" }` 即可。

## 3. 编译与测试

```bash
cd weread-ink
cargo test          # 19 个测试：解析器单测 + 假网关集成测试，无需网络/Key/设备
cargo build --release
```

若 opus 编译报 cmake 版本策略错误，项目已内置 `.cargo/config.toml`
（`CMAKE_POLICY_VERSION_MINIMUM=3.5`）自动处理。

## 4. 预览渲染效果

不需要 API Key、不需要设备，先看面板长什么样：

```bash
cargo run --example sample
open sample.png     # macOS；Linux 用 xdg-open
```

应看到：微信读书标题 + 日期、今日/本周统计、7 天柱状图（今天黑色高亮）、
两本在读图书的灰色进度条。全部为点阵像素风格（12px 等宽点阵字体，阈值二值化，
无抗锯齿），2bpp 四灰阶（黑 / 深灰 / 浅灰 / 白）。

## 5. 配置 API Key 和设备

```bash
cp .env.example .env
```

编辑 `.env`：

```ini
WEREAD_API_KEY=wrk-你的Key          # 必填
WEREAD_DEVICE_TARGET=192.168.1.100  # 设备 IP；省略则自动发现局域网内唯一设备
```

**查设备 IP**：设备「设置 → 网络」可见；或用 mDNS 扫描：
`dns-sd -B _zectrix._udp local.`（macOS 自带），再
`dscacheutil -q host -a name zectrix-<设备ID>.local` 解析出 IP。

`.env` 已被 git 忽略，密钥不会进仓库。CLI 每次启动自动加载它，
也可用系统环境变量代替（环境变量优先）。

先验证取数 + 渲染链路（不碰设备）：

```bash
cargo run --bin weread-ink -- --dry-run dashboard.png
```

成功会打印你的真实数据摘要（今日时长 / 本周 / 在读书目），并生成 dashboard.png。

## 6. 第一次推屏

```bash
cargo run --bin weread-ink
# 或明确指定：cargo run --bin weread-ink -- --target 192.168.1.100
```

看到「已推送到设备」且墨水屏刷新为你的数据即成功。
首次失败常见原因是设备 Wi-Fi 省电（表现为"没有发现可用设备"），程序已内置
3 次重试；仍失败请看[排障手册](#9-排障手册)。

**推送语义**：程序会在显示成功后保持连接 8 秒再退出（设备固件特性，见排障手册），
一次完整推送约 15 秒属正常。

## 7. 定时常驻（推荐）

`--daemon` 模式：建立一次设备连接后**长驻保持**，每 30 分钟自动刷新，
断线自愈、Mac 睡眠唤醒后约 20 秒内自动恢复画面。

先在前台试跑一次确认稳定：

```bash
cargo run --bin weread-ink -- --daemon
```

确认无误后安装为 macOS 后台服务（launchd）：

```bash
cat > ~/Library/LaunchAgents/com.weread.ink.plist << 'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.weread.ink</string>
    <key>ProgramArguments</key>
    <array>
        <string>/Users/你的用户名/ink-projects/weread-ink/target/release/weread-ink</string>
        <string>--daemon</string>
        <string>--interval</string>
        <string>1800</string>
    </array>
    <key>WorkingDirectory</key>
    <string>/Users/你的用户名/ink-projects/weread-ink</string>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>StandardOutPath</key>
    <string>/Users/你的用户名/ink-projects/weread-ink/weread-ink.log</string>
    <key>StandardErrorPath</key>
    <string>/Users/你的用户名/ink-projects/weread-ink/weread-ink.err.log</string>
</dict>
</plist>
EOF

launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/com.weread.ink.plist
```

> 两处 `/Users/你的用户名/ink-projects/weread-ink` 换成你的实际绝对路径
> （`WorkingDirectory` 必须设置，`.env` 按它定位）。

## 8. 日常操作速查

```bash
# 看实时状态（日志全部走 stderr，带时间戳）
tail -f weread-ink.err.log

# 立即刷新一次
launchctl kickstart -k gui/$(id -u)/com.weread.ink

# 停止常驻
launchctl bootout gui/$(id -u)/com.weread.ink

# 改刷新频率：编辑 plist 里 --interval 的值后重新 bootstrap

# 临时手动推一帧（不影响常驻进程时先 bootout，推完再 bootstrap 回来）
./target/release/weread-ink
```

## 9. 排障手册

以下是 2026-10 在 Note 4（固件 3.7.2）上实测得到的设备特性，程序均已内置应对；
此表用于理解日志里的现象：

| 现象 / 日志 | 原因 | 程序的处理 / 你的动作 |
| --- | --- | --- |
| `没有发现可用设备` | 设备 Wi-Fi 深度省电，2 秒探测落空 | 已内置 3 次重试；仍频繁出现则开启设备「待机保持 Wi-Fi」 |
| 推送返回成功但屏幕不动 | **固件特性**：显示提交后立刻断开连接会取消物理刷新 | 已内置：显示成功后保持连接 8 秒再退出；daemon 模式长驻不断 |
| 断开后屏幕恢复旧画面 | **固件特性**：管理连接断开即回滚上一会话画面 | 这就是必须用 daemon 长驻模式的原因 |
| `会话结束：连接已结束` + 自动重连 | 设备不定期掐断会话（省电/固件行为） | daemon 自动 30→10 秒重连并立即补推缓存帧，无需干预 |
| Mac 睡醒后画面缺失约 20 秒 | 睡眠冻结心跳，唤醒后需重新建立会话 | daemon 已做唤醒秒级恢复；等待即可 |
| `Error::UpgradeRequired` | 微信读书 Skill 升级了协议 | 按日志提示升级本项目（watch 上游） |
| `网关响应无效：...` | 网关回包结构变化 | 把报错和日志反馈到项目 issue |

## 10. 个性化

- **换字体**：替换 `fonts/` 下的 TTF（建议 12px 基准点阵字体；任何
  以 12px 为基准的等宽点阵字体都能像素级锐利）
- **改显示档位**：`--format mono|gray4|gray16`（默认 gray4 即 2bpp）
- **改布局/配色**：`src/render.rs` 顶部常量区（`BLACK/DARK/LIGHT` 为
  线性光校准过的三级灰，改动它们可能需要重新校准）
- **改选书逻辑**：`src/weread.rs` 的 `current_books`（当前取最近 14 天内
  读过且未读完的前 2 本）

## 11. 与其他共屏程序的关系

墨水屏同一时刻只有一个管理连接。如果你同时运行其他推屏程序
（如多画面调度器、桌面宠物等），会出现互相挤占：

- 本项目 daemon 启动前，先停止其他占用设备的程序；
- 其他程序要用时，先 `launchctl bootout` 本项目；
- 日志中出现 `设备忙碌` 即表示被其他程序占用，属预期行为，等待下个周期自动恢复。

## 12. 常见问题

**Q：读的时长多久更新到屏上？**
A：微信读书网关侧本身有几分钟延迟；本项目刷新周期默认 30 分钟（`--interval` 可调）。
不建议调到 5 分钟以下——数据源本身是慢变的，高频无意义。

**Q：为什么"累计 X 分钟"看起来比实际少？**
A：面板显示的是单书累计（网关 `recordReadingTime` 字段），跨书合计请看顶部
"本周"统计（`totalReadTime` 口径）。

**Q：有声书/听书算吗？**
A：算时长（网关口径含听书），但书架选书目前只取电子书（`books[]`），
专辑类（`albums[]`）暂未展示。

**Q：耗电如何？**
A：daemon 长驻心跳会保持设备 Wi-Fi 唤醒，实测一夜（约 7 小时）耗电 4%。
介意续航可加大 `--interval` 或改用单次推送模式。

**Q：支持 Windows / Linux 吗？**
A：代码无平台特定依赖，理论可用；字体发现已含三大平台路径，但仅在
Apple Silicon macOS 实机验证过。

---

有任何问题欢迎提 issue。祝阅读愉快 📖
