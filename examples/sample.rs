//! 渲染一张示例仪表盘到 sample.png，不需要 API Key 和设备，用于预览排版。
use chrono::{Datelike, Local};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let today = Local::now().date_naive();
    let monday = today - chrono::Duration::days(today.weekday().num_days_from_monday() as i64);
    let days = [1860u64, 2700, 1320, 0, 2940, 1500, 0]
        .iter()
        .enumerate()
        .map(|(i, &seconds)| weread_ink::DayStat {
            date: monday + chrono::Duration::days(i as i64),
            seconds,
        })
        .collect();
    let dashboard = weread_ink::Dashboard {
        week: weread_ink::WeekStats {
            days,
            total_seconds: 10320,
            read_days: 5,
            compare: Some(0.2),
        },
        books: vec![
            weread_ink::ReadingBook {
                title: "三体Ⅲ·死神永生".into(),
                author: "刘慈欣".into(),
                progress_percent: 62,
                record_seconds: 43200,
                last_read: Some(Local::now()),
            },
            weread_ink::ReadingBook {
                title: "置身事内：中国政府与经济发展".into(),
                author: "兰小欢".into(),
                progress_percent: 24,
                record_seconds: 8100,
                last_read: Some(Local::now() - chrono::Duration::days(1)),
            },
        ],
        generated_at: Local::now(),
    };
    let font = weread_ink::load_font(None)?;
    let png = weread_ink::render_dashboard(&dashboard, &font)?;
    std::fs::write("sample.png", &png)?;
    println!("已生成 sample.png（{} 字节）", png.len());
    Ok(())
}
