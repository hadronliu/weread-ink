# 字体目录

本项目内置 **缝合像素字体 Fusion Pixel 12px（比例字宽，简体中文）**，
由 WOFF 发行包转换回 TTF，原始许可见 `fusion-pixel-LICENSE.txt`（OFL 1.1）。
作者：TakWolf <https://takwolf.com> / <https://github.com/TakWolf/fusion-pixel-font>

渲染模块按以下顺序查找字体：

1. `WEREAD_FONT` 环境变量指定的路径
2. 本目录下的第一个 `.ttf` / `.otf` / `.ttc` 文件（即内置点阵字体）
3. 系统字体（macOS: Hiragino Sans GB / STHeiti / PingFang；Linux: Noto Sans CJK / 文泉驿；Windows: 微软雅黑）

想换其他点阵字体：替换本目录下的 TTF 即可。注意渲染使用**阈值二值化**且字号取
12px 基准的整数倍，任何以 12px 为基准设计的点阵字体都能获得像素级锐利效果。
