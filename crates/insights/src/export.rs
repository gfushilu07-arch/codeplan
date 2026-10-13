//! 报表导出(T-212):Excel(rust_xlsxwriter)与 PDF(printpdf,内嵌系统字体)。

use crate::reports::{AssigneeLoad, BugTrendPoint, CfdPoint, SprintReport, ThroughputPoint};

/// 迭代报告 → xlsx 字节(标题行 + 数据行)。
///
/// # Errors
/// 工作簿写入故障。
pub fn sprint_report_xlsx(r: &SprintReport) -> Result<Vec<u8>, String> {
    use rust_xlsxwriter::Workbook;
    let mut wb = Workbook::new();
    let ws = wb.add_worksheet();
    let _ = ws.write_row(
        0,
        0,
        [
            "total",
            "done",
            "open",
            "points_total",
            "points_done",
            "done_ratio",
        ],
    );
    let _ = ws.write_row(
        1,
        0,
        [
            r.total,
            r.done,
            r.open,
            r.points_total,
            r.points_done,
            r.done_ratio,
        ],
    );
    wb.save_to_buffer().map_err(|e| e.to_string())
}

/// 通用序列表 → xlsx。
///
/// # Errors
/// 工作簿写入故障。
pub fn series_xlsx(headers: [&str; 3], rows: &[(String, i64, i64)]) -> Result<Vec<u8>, String> {
    use rust_xlsxwriter::Workbook;
    let mut wb = Workbook::new();
    let ws = wb.add_worksheet();
    let _ = ws.write_row(0, 0, headers);
    for (i, (label, a, b)) in rows.iter().enumerate() {
        let _ = ws.write(i as u32 + 1, 0, label);
        let _ = ws.write(i as u32 + 1, 1, *a);
        let _ = ws.write(i as u32 + 1, 2, *b);
    }
    wb.save_to_buffer().map_err(|e| e.to_string())
}

/// 报表文本行 → PDF 字节(内嵌系统 TTF 支持中文;PingFang/Noto 候选)。
///
/// # Errors
/// PDF 生成故障(未找到可用字体)。
pub fn text_pdf(title: &str, lines: &[String]) -> Result<Vec<u8>, String> {
    use printpdf::*;
    let font_candidates: [(&str, u32); 4] = [
        ("/System/Library/Fonts/PingFang.ttc", 0),
        ("/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc", 0),
        ("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf", 0),
        ("/System/Library/Fonts/Helvetica.ttc", 0),
    ];
    for (path, index) in font_candidates {
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        let mut font_warnings = Vec::new();
        let Some(font) = ParsedFont::from_bytes(
            &bytes,
            usize::try_from(index).unwrap_or(0),
            &mut font_warnings,
        ) else {
            continue;
        };
        let mut doc = PdfDocument::new("codeplan-report");
        let font_id = doc.add_font(&font);
        let mut ops = vec![
            Op::StartTextSection,
            Op::SetFont {
                font: PdfFontHandle::External(font_id.clone()),
                size: Pt(16.0),
            },
            Op::SetTextCursor {
                pos: Point {
                    x: Mm(20.0).into(),
                    y: Mm(275.0).into(),
                },
            },
            Op::ShowText {
                items: vec![TextItem::Text(title.to_owned())],
            },
            Op::AddLineBreak,
            Op::SetFont {
                font: PdfFontHandle::External(font_id.clone()),
                size: Pt(9.0),
            },
            Op::SetLineHeight { lh: Pt(12.0) },
        ];
        for line in lines {
            ops.push(Op::ShowText {
                items: vec![TextItem::Text(line.clone())],
            });
            ops.push(Op::AddLineBreak);
        }
        ops.push(Op::EndTextSection);
        let page = PdfPage::new(Mm(210.0), Mm(297.0), ops);
        let mut warnings = Vec::new();
        let bytes = doc
            .with_pages(vec![page])
            .save(&PdfSaveOptions::default(), &mut warnings);
        return Ok(bytes);
    }
    Err("未找到可用 TTF 字体,PDF 导出失败".to_owned())
}

/// 组装迭代报告 PDF 文本行。
#[must_use]
pub fn sprint_report_lines(r: &SprintReport) -> Vec<String> {
    vec![
        "Sprint Report".to_owned(),
        format!("total: {}", r.total),
        format!("done: {}", r.done),
        format!("open: {}", r.open),
        format!("points_total: {}", r.points_total),
        format!("points_done: {}", r.points_done),
        format!("done_ratio: {}%", r.done_ratio),
    ]
}

/// 趋势/吞吐 → 序列行(label, created/done, closed/week-count)。
#[must_use]
pub fn trend_lines(trend: &[BugTrendPoint]) -> Vec<String> {
    std::iter::once("date created closed".to_owned())
        .chain(
            trend
                .iter()
                .map(|p| format!("{} {} {}", p.date, p.created, p.closed)),
        )
        .collect()
}

/// 累计流 → 序列行。
#[must_use]
pub fn cfd_lines(cfd: &[CfdPoint]) -> Vec<String> {
    std::iter::once("date created done wip".to_owned())
        .chain(
            cfd.iter()
                .map(|p| format!("{} {} {} {}", p.date, p.created, p.done, p.wip)),
        )
        .collect()
}

/// 吞吐 → 序列行。
#[must_use]
pub fn throughput_lines(t: &[ThroughputPoint]) -> Vec<String> {
    std::iter::once("week done".to_owned())
        .chain(t.iter().map(|p| format!("{} {}", p.week, p.done)))
        .collect()
}

/// 负载 → 序列行。
#[must_use]
pub fn load_lines(l: &[AssigneeLoad]) -> Vec<String> {
    std::iter::once("assignee open_items open_points".to_owned())
        .chain(
            l.iter()
                .map(|p| format!("{} {} {}", p.assignee, p.open_items, p.open_points)),
        )
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xlsx_roundtrip_bytes() {
        let r = SprintReport {
            total: 3,
            done: 2,
            open: 1,
            points_total: 10,
            points_done: 8,
            done_ratio: 67,
        };
        let bytes = sprint_report_xlsx(&r).unwrap();
        assert!(bytes.len() > 100, "xlsx 应为非空 zip");
        assert_eq!(&bytes[..2], b"PK");
    }

    #[test]
    fn series_xlsx_ok() {
        let bytes = series_xlsx(
            ["date", "created", "closed"],
            &[("2026-10-11".into(), 3, 1), ("2026-10-10".into(), 0, 0)],
        )
        .unwrap();
        assert_eq!(&bytes[..2], b"PK");
    }

    #[test]
    fn pdf_with_system_font() {
        let lines = sprint_report_lines(&SprintReport {
            total: 3,
            done: 2,
            open: 1,
            points_total: 10,
            points_done: 8,
            done_ratio: 67,
        });
        match text_pdf("report", &lines) {
            Ok(bytes) => assert!(bytes.starts_with(b"%PDF")),
            Err(e) => panic!("PDF 导出失败: {e}"),
        }
    }

    #[test]
    fn line_builders() {
        let trend = vec![BugTrendPoint {
            date: "2026-10-11".into(),
            created: 3,
            closed: 1,
        }];
        assert_eq!(trend_lines(&trend)[1], "2026-10-11 3 1");
    }
}
