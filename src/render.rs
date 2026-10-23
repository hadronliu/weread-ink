//! 400×300 仪表盘渲染。纯函数：Dashboard + 字体字节 → PNG 字节。
//! 2bpp（Gray4）四灰阶设计：文字用点阵字体阈值二值化（纯黑白），
//! 进度条与图表用灰阶；灰阶取值经过线性光校准，恰好落在设备 gray4 调色板上：
//! 黑 0 / 深灰 105(≈线性40) / 浅灰 210(≈线性185) / 白 255。
use crate::{Dashboard, Error, Result};
use ab_glyph::{point, Font as _, FontRef, Glyph, PxScaleFont, ScaleFont as _};
use chrono::{Datelike, Local};
use image::{ImageFormat, Rgb, RgbImage};
use std::io::Cursor;

pub(crate) const WIDTH: u32 = 400;
pub(crate) const HEIGHT: u32 = 300;
const BLACK: u8 = 0;
const DARK: u8 = 105;
const LIGHT: u8 = 210;
const MARGIN: u32 = 16;
const CHART_TOP: u32 = 116;
const CHART_BOTTOM: u32 = 164; // 柱区底边（不含星期标签）
const CHART_MAX_BAR: u32 = 46;
/// 点阵字体基准 12px：所有字号取整数倍，阈值渲染时保持像素完美。
const FONT_BASE: f32 = 12.0;
const WEEKDAY_LABELS: [&str; 7] = ["一", "二", "三", "四", "五", "六", "日"];

pub(crate) fn render(dashboard: &Dashboard, font_bytes: &[u8]) -> Result<Vec<u8>> {
    let bad = |e: String| Error::Render(e);
    let font = FontRef::try_from_slice_and_index(font_bytes, 0)
        .map_err(|e| bad(format!("字体无法解析：{e}")))?;
    let mut img = RgbImage::from_pixel(WIDTH, HEIGHT, Rgb([255, 255, 255]));

    draw_header(&mut img, &font, dashboard);
    draw_stats(&mut img, &font, dashboard);
    draw_chart(&mut img, &font, dashboard);
    draw_books(&mut img, &font, dashboard);

    let mut png = Vec::new();
    img.write_to(&mut Cursor::new(&mut png), ImageFormat::Png)
        .map_err(|e| bad(e.to_string()))?;
    Ok(png)
}

fn scale<'a>(font: &FontRef<'a>, mul: f32) -> PxScaleFont<FontRef<'a>> {
    font.clone().into_scaled(FONT_BASE * mul)
}

fn draw_header(img: &mut RgbImage, font: &FontRef, d: &Dashboard) {
    let title = scale(font, 2.0);
    draw_text(img, &title, "微信读书", MARGIN as f32, 30.0, BLACK);
    let weekday = WEEKDAY_LABELS[d.generated_at.weekday().num_days_from_monday() as usize];
    let date = format!("{} 周{}", d.generated_at.format("%m-%d"), weekday);
    let date_font = scale(font, 1.0);
    let w = text_width(&date_font, &date);
    draw_text(img, &date_font, &date, (WIDTH - MARGIN) as f32 - w, 26.0, DARK);
    hline(img, 40, MARGIN, WIDTH - MARGIN, LIGHT);
}

fn draw_stats(img: &mut RgbImage, font: &FontRef, d: &Dashboard) {
    let label = scale(font, 1.0);
    let big = scale(font, 3.0);
    draw_text(img, &label, "今日", MARGIN as f32, 58.0, DARK);
    draw_text(img, &big, &crate::format_duration(d.today_seconds()), MARGIN as f32, 94.0, BLACK);
    let right = (WIDTH - MARGIN) as f32;
    let mut line = |text: String, baseline: f32| {
        let w = text_width(&label, &text);
        draw_text(img, &label, &text, right - w, baseline, DARK);
    };
    line(format!("本周 {}", crate::format_duration(d.week.total_seconds)), 58.0);
    line(format!("阅读 {} 天", d.week.read_days), 74.0);
    let compare = match d.week.compare {
        Some(c) => format!("日均较上周 {}{:.0}%", if c >= 0.0 { "+" } else { "-" }, c.abs() * 100.0),
        None => "日均较上周 —".into(),
    };
    line(compare, 90.0);
}

/// 本周柱状图：今天黑色高亮，过去深灰，未来只画浅灰轨道。
fn draw_chart(img: &mut RgbImage, font: &FontRef, d: &Dashboard) {
    let today = Local::now().date_naive();
    let inner = (WIDTH - 2 * MARGIN) as f32;
    let slot = inner / 7.0;
    let bar_w = 30.0;
    let max = d.week.days.iter().map(|x| x.seconds).max().unwrap_or(1).max(1);
    for (i, day) in d.week.days.iter().enumerate() {
        let x0 = MARGIN as f32 + slot * i as f32 + (slot - bar_w) / 2.0;
        rect(img, x0, CHART_TOP as f32, x0 + bar_w, CHART_BOTTOM as f32, LIGHT);
        let is_today = day.date == today;
        if day.seconds == 0 && !is_today {
            continue;
        }
        let h = bar_height(day.seconds, max);
        let top = CHART_BOTTOM as f32 - h as f32;
        rect(img, x0, top, x0 + bar_w, CHART_BOTTOM as f32, if is_today { BLACK } else { DARK });
        if is_today && day.seconds > 0 {
            let value = scale(font, 1.0);
            let text = format!("{}分", day.seconds / 60);
            let w = text_width(&value, &text);
            draw_text(img, &value, &text, x0 + bar_w / 2.0 - w / 2.0, top - 4.0, DARK);
        }
    }
    let label_font = scale(font, 1.0);
    for (i, name) in WEEKDAY_LABELS.iter().enumerate() {
        let cx = MARGIN as f32 + slot * i as f32 + slot / 2.0;
        let w = text_width(&label_font, name);
        draw_text(img, &label_font, name, cx - w / 2.0, (CHART_BOTTOM + 16) as f32, DARK);
    }
    hline(img, 192, MARGIN, WIDTH - MARGIN, LIGHT);
}

fn draw_books(img: &mut RgbImage, font: &FontRef, d: &Dashboard) {
    if d.books.is_empty() {
        let hint = scale(font, 1.0);
        let text = "书架暂无最近在读的书";
        let w = text_width(&hint, text);
        draw_text(img, &hint, text, (WIDTH as f32 - w) / 2.0, 250.0, DARK);
        return;
    }
    let title_font = scale(font, 2.0);
    let meta_font = scale(font, 1.0);
    for (i, book) in d.books.iter().take(2).enumerate() {
        let block_top = 194 + i as u32 * 50;
        let title = truncate_to_width(&title_font, &format!("《{}》", book.title), 270.0);
        draw_text(img, &title_font, &title, MARGIN as f32, (block_top + 22) as f32, BLACK);
        let author = truncate_to_width(&meta_font, &book.author, 96.0);
        let w = text_width(&meta_font, &author);
        draw_text(
            img,
            &meta_font,
            &author,
            (WIDTH - MARGIN) as f32 - w,
            (block_top + 16) as f32,
            DARK,
        );
        let bar_y = block_top + 30;
        let full = (WIDTH - 2 * MARGIN) as f32;
        // 进度条按需求用灰阶：浅灰轨道 + 深灰填充。
        rect(img, MARGIN as f32, bar_y as f32, (WIDTH - MARGIN) as f32, (bar_y + 8) as f32, LIGHT);
        let fill = full * book.progress_percent as f32 / 100.0;
        rect(img, MARGIN as f32, bar_y as f32, MARGIN as f32 + fill, (bar_y + 8) as f32, DARK);
        let last = match book.last_read {
            Some(t) => relative_time(t, d.generated_at),
            None => "—".into(),
        };
        let meta = format!(
            "{}% · 累计 {} · {}",
            book.progress_percent,
            crate::format_duration(book.record_seconds),
            last
        );
        draw_text(img, &meta_font, &meta, MARGIN as f32, (bar_y + 22) as f32, DARK);
    }
}

/// 柱高：0 也有 3px 底座，最大 CHART_MAX_BAR，与最大值成比例。
pub(crate) fn bar_height(seconds: u64, max: u64) -> u32 {
    if seconds == 0 {
        return 3;
    }
    let ratio = seconds as f32 / max as f32;
    (3.0 + ratio * (CHART_MAX_BAR as f32 - 3.0)).round() as u32
}

fn relative_time(t: chrono::DateTime<Local>, now: chrono::DateTime<Local>) -> String {
    let date = t.date_naive();
    let today = now.date_naive();
    if date == today {
        format!("今天 {}", t.format("%H:%M"))
    } else if date == today - chrono::Duration::days(1) {
        format!("昨天 {}", t.format("%H:%M"))
    } else {
        format!("{}", t.format("%m-%d %H:%M"))
    }
}

fn truncate_to_width(font: &PxScaleFont<FontRef>, text: &str, max_w: f32) -> String {
    if text_width(font, text) <= max_w {
        return text.into();
    }
    let mut out = String::new();
    for ch in text.chars() {
        if text_width(font, &format!("{out}{ch}…")) > max_w {
            return format!("{out}…");
        }
        out.push(ch);
    }
    out
}

fn text_width(font: &PxScaleFont<FontRef>, text: &str) -> f32 {
    text.chars().map(|ch| font.h_advance(font.glyph_id(ch))).sum()
}

/// 阈值二值化文字：覆盖率 ≥ 0.5 记为全量墨色，否则留白。
/// 配合 12px 基准点阵字体的整数倍字号，得到无抗锯齿的锐利像素字。
fn draw_text(img: &mut RgbImage, font: &PxScaleFont<FontRef>, text: &str, x: f32, baseline: f32, ink: u8) {
    let mut pen = x;
    for ch in text.chars() {
        let mut glyph: Glyph = font.scaled_glyph(ch);
        glyph.position = point(pen, baseline);
        if let Some(outlined) = font.outline_glyph(glyph) {
            let bounds = outlined.px_bounds();
            outlined.draw(|dx, dy, coverage| {
                if coverage < 0.5 {
                    return;
                }
                let px = bounds.min.x as i64 + dx as i64;
                let py = bounds.min.y as i64 + dy as i64;
                if px < 0 || py < 0 || px >= WIDTH as i64 || py >= HEIGHT as i64 {
                    return;
                }
                img.put_pixel(px as u32, py as u32, Rgb([ink, ink, ink]));
            });
        }
        pen += font.h_advance(font.glyph_id(ch));
    }
}

fn rect(img: &mut RgbImage, x0: f32, y0: f32, x1: f32, y1: f32, gray: u8) {
    let (x0, x1) = (x0.max(0.0).min(WIDTH as f32) as u32, x1.max(0.0).min(WIDTH as f32) as u32);
    let (y0, y1) = (y0.max(0.0).min(HEIGHT as f32) as u32, y1.max(0.0).min(HEIGHT as f32) as u32);
    for y in y0..y1 {
        for x in x0..x1 {
            img.put_pixel(x, y, Rgb([gray, gray, gray]));
        }
    }
}

fn hline(img: &mut RgbImage, y: u32, x0: u32, x1: u32, gray: u8) {
    for x in x0..x1 {
        img.put_pixel(x, y, Rgb([gray, gray, gray]));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DayStat, ReadingBook, WeekStats};

    fn sample_dashboard(day_seconds: [u64; 7]) -> Dashboard {
        let today = Local::now().date_naive();
        let monday = today - chrono::Duration::days(today.weekday().num_days_from_monday() as i64);
        Dashboard {
            week: WeekStats {
                days: (0..7usize)
                    .map(|i| DayStat {
                        date: monday + chrono::Duration::days(i as i64),
                        seconds: day_seconds[i],
                    })
                    .collect(),
                total_seconds: day_seconds.iter().sum(),
                read_days: 3,
                compare: Some(0.2),
            },
            books: vec![ReadingBook {
                title: "三体".into(),
                author: "刘慈欣".into(),
                progress_percent: 62,
                record_seconds: 43200,
                last_read: Some(Local::now()),
            }],
            generated_at: Local::now(),
        }
    }

    fn load_font_or_skip() -> Option<Vec<u8>> {
        match crate::font::load(None) {
            Ok(f) => Some(f),
            Err(_) => {
                eprintln!("跳过渲染测试：本机无中文字体");
                None
            }
        }
    }

    #[test]
    fn render_produces_400x300_png() {
        let Some(font) = load_font_or_skip() else { return };
        let png = render(&sample_dashboard([1860, 2700, 0, 0, 0, 0, 0]), &font).unwrap();
        let img = image::load_from_memory(&png).unwrap();
        assert_eq!((img.width(), img.height()), (400, 300));
    }

    #[test]
    fn bars_scale_with_values_and_today_is_darkest() {
        let Some(font) = load_font_or_skip() else { return };
        let mut days = [0u64; 7];
        days[0] = 3600;
        days[1] = 1800;
        let today = Local::now().date_naive();
        let monday = today - chrono::Duration::days(today.weekday().num_days_from_monday() as i64);
        let today_idx = (today - monday).num_days() as usize;
        days[today_idx] = days[today_idx].max(600);
        let png = render(&sample_dashboard(days), &font).unwrap();
        let img = image::load_from_memory(&png).unwrap().to_rgb8();
        let slot = (400 - 32) as f32 / 7.0;
        let column_dark = |slot_idx: usize, gray_cap: u8| -> u32 {
            let cx = (16.0 + slot * slot_idx as f32 + slot / 2.0) as u32;
            (CHART_TOP..CHART_BOTTOM)
                .filter(|&y| img.get_pixel(cx, y)[0] < gray_cap)
                .count() as u32
        };
        let h0 = column_dark(0, 160);
        let h1 = column_dark(1, 160);
        assert!(h0 > h1, "两倍时长柱高应更高：{h0} vs {h1}");
        // 今天必须纯黑（0），过去柱是深灰（105），浅灰轨道不计入。
        let h_today = column_dark(today_idx, 60);
        assert!(h_today > 0, "今天的柱应为黑色高亮");
        let light_row = column_dark(today_idx, 220);
        assert!(light_row > h_today, "轨道浅灰应比黑柱高");
    }

    /// 进度条填充必须是深灰（非纯黑、非白）。
    #[test]
    fn progress_bar_fill_is_dark_gray() {
        let Some(font) = load_font_or_skip() else { return };
        let png = render(&sample_dashboard([1860, 2700, 0, 0, 0, 0, 0]), &font).unwrap();
        let img = image::load_from_memory(&png).unwrap().to_rgb8();
        let bar_y = 194 + 30 + 3; // 第一本书进度条垂直中部
        let fill = img.get_pixel(20, bar_y)[0];
        let track = img.get_pixel(380, bar_y)[0];
        assert_eq!(fill, DARK, "进度条填充应为深灰");
        assert_eq!(track, LIGHT, "进度条轨道应为浅灰");
    }

    /// 文字二值化后，面板中不应出现黑与深灰之间的中间墨色。
    #[test]
    fn text_rendering_is_binary() {
        let Some(font) = load_font_or_skip() else { return };
        let png = render(&sample_dashboard([1860, 2700, 0, 0, 0, 0, 0]), &font).unwrap();
        let img = image::load_from_memory(&png).unwrap().to_rgb8();
        // 标题区（避开分隔线与图表）
        for y in 4..36 {
            for x in 0..WIDTH {
                let v = img.get_pixel(x, y)[0];
                assert!(
                    v == 0 || v == DARK || v == LIGHT || v == 255,
                    "y={y} x={x} 出现中间灰 {v}"
                );
            }
        }
    }

    /// 两本书都展示时，底部必须留白，不允许任何内容贴到 300px 边界。
    #[test]
    fn two_books_keep_bottom_margin_clear() {
        let Some(font) = load_font_or_skip() else { return };
        let mut dashboard = sample_dashboard([1860, 2700, 0, 0, 0, 0, 0]);
        dashboard.books.push(ReadingBook {
            title: "置身事内".into(),
            author: "兰小欢".into(),
            progress_percent: 24,
            record_seconds: 8100,
            last_read: Some(Local::now()),
        });
        let png = render(&dashboard, &font).unwrap();
        let img = image::load_from_memory(&png).unwrap().to_rgb8();
        for y in (HEIGHT - 3)..HEIGHT {
            for x in 0..WIDTH {
                assert_eq!(img.get_pixel(x, y)[0], 255, "底部第 {y} 行在 x={x} 处有内容");
            }
        }
    }

    #[test]
    fn bar_height_is_proportional_with_floor() {
        assert_eq!(bar_height(0, 3600), 3);
        assert_eq!(bar_height(3600, 3600), CHART_MAX_BAR);
        let half = bar_height(1800, 3600);
        assert!((18..=28).contains(&half), "半量柱高应在中间附近：{half}");
    }
}
