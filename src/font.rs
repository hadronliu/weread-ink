//! 中文字体发现：显式路径 → ./fonts/ 目录 → 系统字体候选。
use crate::{Error, Result};
use std::path::{Path, PathBuf};

/// macOS 优先 Hiragino（体积小、简体覆盖好），Linux/Windows 给出常见候选。
const SYSTEM_CANDIDATES: &[&str] = &[
    "/System/Library/Fonts/Hiragino Sans GB.ttc",
    "/System/Library/Fonts/STHeiti Medium.ttc",
    "/System/Library/Fonts/STHeiti Light.ttc",
    "/System/Library/Fonts/PingFang.ttc",
    "/System/Library/Fonts/Songti.ttc",
    "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
    "C:\\Windows\\Fonts\\msyh.ttc",
    "C:\\Windows\\Fonts\\simhei.ttf",
];

pub(crate) fn load(preferred: Option<&Path>) -> Result<Vec<u8>> {
    let mut tried = Vec::new();
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(p) = preferred {
        candidates.push(p.to_path_buf());
    }
    if let Ok(entries) = std::fs::read_dir("fonts") {
        let mut found: Vec<PathBuf> = entries
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                matches!(
                    p.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref(),
                    Some("ttf") | Some("otf") | Some("ttc")
                )
            })
            .collect();
        found.sort();
        candidates.extend(found);
    }
    candidates.extend(SYSTEM_CANDIDATES.iter().map(PathBuf::from));
    for path in candidates {
        tried.push(path.clone());
        match std::fs::read(&path) {
            // 空文件或过小的碎片直接视为不可用，继续尝试下一个候选。
            Ok(bytes) if bytes.len() > 1024 => return Ok(bytes),
            _ => continue,
        }
    }
    Err(Error::NoFont)
}
