use calamine::{Data, DataType};
use rust_xlsxwriter::{
    Color, ConditionalFormatFormula, Format, Workbook, Worksheet, column_number_to_name,
};

use super::data::{CellKind, ReportColumn};
use super::reader::{cell_to_decimal, cell_to_string};
use super::reconciler::ReconciledTransaction;
use super::styles::StylePool;
use super::summary_data::{
    BrandSummary, FailedUploadExtra, FailedUploadRecord, MetricRow, SummaryRatioItem,
};

pub struct ColumnGroup<'a> {
    pub title: &'a str,
    pub subheaders: &'a [&'a str],
}

fn write_date_cell(
    ws: &mut Worksheet,
    row: u32,
    col: u16,
    cell: &Data,
    format: &Format,
) -> Result<(), String> {
    if matches!(cell, Data::Empty) {
        return ws
            .write_string_with_format(row, col, "", format)
            .map(|_| ())
            .map_err(|e| e.to_string());
    }

    if let Some(value) = cell.as_datetime() {
        return ws
            .write_datetime_with_format(row, col, value, format)
            .map(|_| ())
            .map_err(|e| e.to_string());
    }

    let value = cell_to_string(cell);
    if let Some(datetime) = crate::utils::dates::parse_date_text(&value) {
        return ws
            .write_datetime_with_format(row, col, datetime, format)
            .map(|_| ())
            .map_err(|e| e.to_string());
    }
    eprintln!(
        "[警告] 工作表“{}”第 {} 行第 {} 列日期值 '{}' 无法解析，已保留原文本。",
        ws.name(),
        row + 1,
        col + 1,
        value
    );
    ws.write_string_with_format(row, col, value, format)
        .map(|_| ())
        .map_err(|e| e.to_string())
}

fn write_decimal_cell(
    ws: &mut Worksheet,
    row: u32,
    col: u16,
    cell: &Data,
    format: &Format,
    empty_format: &Format,
) -> Result<(), String> {
    match cell_to_decimal(cell) {
        Some(value) => ws.write_with_format(row, col, value, format),
        None => ws.write_string_with_format(row, col, "", empty_format),
    }
    .map(|_| ())
    .map_err(|e| e.to_string())
}

impl CellKind {
    pub fn write(
        self,
        ws: &mut Worksheet,
        s: &StylePool,
        row: u32,
        col: u16,
        cell: &Data,
    ) -> Result<(), String> {
        match self {
            Self::Date => write_date_cell(ws, row, col, cell, &s.date),
            Self::DateTime => write_date_cell(ws, row, col, cell, &s.datetime),
            Self::Money => write_decimal_cell(ws, row, col, cell, &s.money, &s.text_right),
            Self::Percent => write_decimal_cell(ws, row, col, cell, &s.percent, &s.text_right),
            Self::Text | Self::CenteredText => {
                let format = if matches!(self, Self::CenteredText) {
                    &s.text_center
                } else {
                    &s.text_left
                };
                ws.write_string_with_format(row, col, cell_to_string(cell), format)
                    .map(|_| ())
                    .map_err(|e| e.to_string())
            }
        }
    }
}

fn write_report_row(
    ws: &mut Worksheet,
    styles: &StylePool,
    index: u32,
    row: &[Data],
    columns: &[ReportColumn],
) -> Result<(), String> {
    ws.set_row_height(index, 22.0).map_err(|e| e.to_string())?;
    for (col, column) in columns.iter().enumerate() {
        column
            .kind
            .write(ws, styles, index, col as u16, column.cell(row))?;
    }
    Ok(())
}

/// 报表工作表构建器：统一封装工作表初始化、列宽配置、标题头、章节头及行流式写入
pub struct ReportSheetBuilder<'a> {
    ws: &'a mut Worksheet,
    styles: &'a StylePool,
    cur_row: u32,
}

impl<'a> ReportSheetBuilder<'a> {
    pub fn new(wb: &'a mut Workbook, name: &str, styles: &'a StylePool) -> Result<Self, String> {
        let ws = wb.add_worksheet();
        ws.set_name(name).map_err(|e| e.to_string())?;
        Ok(Self {
            ws,
            styles,
            cur_row: 0,
        })
    }

    #[cfg(test)]
    pub fn cur_row(&self) -> u32 {
        self.cur_row
    }

    pub fn set_column_widths(&mut self, widths: &[f64]) -> Result<&mut Self, String> {
        for (col, &w) in widths.iter().enumerate() {
            self.ws
                .set_column_width(col as u16, w)
                .map_err(|e| e.to_string())?;
        }
        Ok(self)
    }

    pub fn set_freeze_panes(&mut self, row: u32, col: u16) -> Result<&mut Self, String> {
        self.ws
            .set_freeze_panes(row, col)
            .map_err(|e| e.to_string())?;
        Ok(self)
    }

    pub fn write_title_block(
        &mut self,
        max_col: u16,
        title: &str,
        subtitle: &str,
    ) -> Result<&mut Self, String> {
        self.ws.set_row_height(0, 30.0).map_err(|e| e.to_string())?;
        self.ws
            .merge_range(0, 0, 0, max_col, title, &self.styles.title)
            .map_err(|e| e.to_string())?;

        self.ws.set_row_height(1, 22.0).map_err(|e| e.to_string())?;
        self.ws
            .merge_range(1, 0, 1, max_col, subtitle, &self.styles.subtitle)
            .map_err(|e| e.to_string())?;

        self.ws.set_row_height(2, 8.0).map_err(|e| e.to_string())?;
        self.cur_row = 3;
        Ok(self)
    }

    pub fn write_section_header(&mut self, max_col: u16, title: &str) -> Result<&mut Self, String> {
        self.ws
            .set_row_height(self.cur_row, 24.0)
            .map_err(|e| e.to_string())?;
        self.ws
            .merge_range(
                self.cur_row,
                0,
                self.cur_row,
                max_col,
                title,
                &self.styles.section_header,
            )
            .map_err(|e| e.to_string())?;
        self.cur_row += 1;
        Ok(self)
    }

    pub fn write_headers(&mut self, headers: &[&str]) -> Result<&mut Self, String> {
        self.write_headers_with(headers, |_| false)
    }

    pub fn write_headers_with<F>(
        &mut self,
        headers: &[&str],
        mut is_center: F,
    ) -> Result<&mut Self, String>
    where
        F: FnMut(usize) -> bool,
    {
        self.ws
            .set_row_height(self.cur_row, 30.0)
            .map_err(|e| e.to_string())?;
        for (col, h) in headers.iter().enumerate() {
            let fmt = if is_center(col) {
                &self.styles.col_header_center
            } else {
                &self.styles.col_header
            };
            self.ws
                .write_string_with_format(self.cur_row, col as u16, *h, fmt)
                .map_err(|e| e.to_string())?;
        }
        self.cur_row += 1;
        Ok(self)
    }

    pub fn write_blank_row(&mut self, height: f64) -> Result<&mut Self, String> {
        self.ws
            .set_row_height(self.cur_row, height)
            .map_err(|e| e.to_string())?;
        self.cur_row += 1;
        Ok(self)
    }

    pub fn set_column_width(&mut self, col: u16, width: f64) -> Result<&mut Self, String> {
        self.ws
            .set_column_width(col, width)
            .map_err(|e| e.to_string())?;
        Ok(self)
    }

    pub fn write_grouped_column_headers(
        &mut self,
        groups: &[ColumnGroup<'_>],
    ) -> Result<&mut Self, String> {
        self.ws
            .set_row_height(self.cur_row, 30.0)
            .map_err(|e| e.to_string())?;
        self.ws
            .set_row_height(self.cur_row + 1, 30.0)
            .map_err(|e| e.to_string())?;

        let mut col: u16 = 0;
        for group in groups {
            if group.subheaders.is_empty() {
                self.ws
                    .merge_range(
                        self.cur_row,
                        col,
                        self.cur_row + 1,
                        col,
                        group.title,
                        &self.styles.col_header_center,
                    )
                    .map_err(|e| e.to_string())?;
                col += 1;
            } else {
                let span = group.subheaders.len() as u16;
                if span > 1 {
                    self.ws
                        .merge_range(
                            self.cur_row,
                            col,
                            self.cur_row,
                            col + span - 1,
                            group.title,
                            &self.styles.col_header_center,
                        )
                        .map_err(|e| e.to_string())?;
                } else {
                    self.ws
                        .write_string_with_format(
                            self.cur_row,
                            col,
                            group.title,
                            &self.styles.col_header_center,
                        )
                        .map_err(|e| e.to_string())?;
                }
                for (sub_idx, subheader) in group.subheaders.iter().enumerate() {
                    self.ws
                        .write_string_with_format(
                            self.cur_row + 1,
                            col + sub_idx as u16,
                            *subheader,
                            &self.styles.col_header_center,
                        )
                        .map_err(|e| e.to_string())?;
                }
                col += span;
            }
        }
        self.cur_row += 2;
        Ok(self)
    }

    pub fn write_tip_row(&mut self, max_col: u16, text: &str) -> Result<&mut Self, String> {
        self.ws
            .set_row_height(self.cur_row, 22.0)
            .map_err(|e| e.to_string())?;
        self.ws
            .merge_range(
                self.cur_row,
                0,
                self.cur_row,
                max_col,
                text,
                &self.styles.tip_row,
            )
            .map_err(|e| e.to_string())?;
        self.cur_row += 1;
        Ok(self)
    }

    pub fn write_notes(&mut self, max_col: u16, notes: &[&str]) -> Result<&mut Self, String> {
        for note in notes {
            self.ws
                .set_row_height(self.cur_row, 28.0)
                .map_err(|e| e.to_string())?;
            self.ws
                .merge_range(
                    self.cur_row,
                    0,
                    self.cur_row,
                    max_col,
                    note,
                    &self.styles.text_left,
                )
                .map_err(|e| e.to_string())?;
            self.cur_row += 1;
        }
        Ok(self)
    }

    pub fn write_metric_count_row(
        &mut self,
        name: &str,
        row: &MetricRow,
    ) -> Result<&mut Self, String> {
        self.ws
            .set_row_height(self.cur_row, 22.0)
            .map_err(|e| e.to_string())?;
        self.ws
            .write_string_with_format(self.cur_row, 0, name, &self.styles.text_left)
            .map_err(|e| e.to_string())?;
        self.ws
            .write_with_format(self.cur_row, 1, row.app_amt_wan(), &self.styles.money)
            .map_err(|e| e.to_string())?;
        self.ws
            .write_number_with_format(self.cur_row, 2, row.app_cnt as f64, &self.styles.int_count)
            .map_err(|e| e.to_string())?;
        self.ws
            .write_with_format(self.cur_row, 3, row.dig_amt_wan(), &self.styles.money)
            .map_err(|e| e.to_string())?;
        self.ws
            .write_number_with_format(self.cur_row, 4, row.dig_cnt as f64, &self.styles.int_count)
            .map_err(|e| e.to_string())?;
        self.ws
            .write_with_format(self.cur_row, 5, row.tot_amt_wan(), &self.styles.money)
            .map_err(|e| e.to_string())?;
        self.ws
            .write_number_with_format(
                self.cur_row,
                6,
                row.tot_cnt() as f64,
                &self.styles.int_count,
            )
            .map_err(|e| e.to_string())?;
        self.cur_row += 1;
        Ok(self)
    }

    pub fn write_metric_ratio_row(&mut self, item: &SummaryRatioItem) -> Result<&mut Self, String> {
        self.ws
            .set_row_height(self.cur_row, 24.0)
            .map_err(|e| e.to_string())?;
        self.ws
            .write_string_with_format(self.cur_row, 0, item.name, &self.styles.text_left)
            .map_err(|e| e.to_string())?;
        self.ws
            .write_with_format(self.cur_row, 1, item.app_amt_wan(), &self.styles.money)
            .map_err(|e| e.to_string())?;
        self.ws
            .write_number_with_format(self.cur_row, 2, item.app_pct, &self.styles.percent)
            .map_err(|e| e.to_string())?;
        self.ws
            .write_with_format(self.cur_row, 3, item.dig_amt_wan(), &self.styles.money)
            .map_err(|e| e.to_string())?;
        self.ws
            .write_number_with_format(self.cur_row, 4, item.dig_pct, &self.styles.percent)
            .map_err(|e| e.to_string())?;
        self.ws
            .write_with_format(self.cur_row, 5, item.tot_amt_wan(), &self.styles.money)
            .map_err(|e| e.to_string())?;
        self.ws
            .write_number_with_format(self.cur_row, 6, item.tot_pct, &self.styles.percent)
            .map_err(|e| e.to_string())?;
        self.cur_row += 1;
        Ok(self)
    }

    pub fn write_category_brand_group(
        &mut self,
        category: &str,
        brands: &[BrandSummary],
    ) -> Result<&mut Self, String> {
        let start_row = self.cur_row;
        for brand in brands {
            self.ws
                .set_row_height(self.cur_row, 22.0)
                .map_err(|e| e.to_string())?;
            self.ws
                .write_string_with_format(self.cur_row, 0, category, &self.styles.text_center)
                .map_err(|e| e.to_string())?;
            self.ws
                .write_string_with_format(
                    self.cur_row,
                    1,
                    brand.brand.as_str(),
                    &self.styles.text_left,
                )
                .map_err(|e| e.to_string())?;

            for s_idx in 0..5 {
                let col_amt = 2 + (s_idx as u16) * 2;
                let col_cnt = col_amt + 1;

                let total = brand.totals[s_idx];
                if total.is_empty() {
                    self.ws
                        .write_string_with_format(
                            self.cur_row,
                            col_amt,
                            "-",
                            &self.styles.text_right,
                        )
                        .map_err(|e| e.to_string())?;
                    self.ws
                        .write_string_with_format(
                            self.cur_row,
                            col_cnt,
                            "-",
                            &self.styles.text_right,
                        )
                        .map_err(|e| e.to_string())?;
                } else {
                    self.ws
                        .write_with_format(self.cur_row, col_amt, total.amount, &self.styles.money)
                        .map_err(|e| e.to_string())?;
                    self.ws
                        .write_number_with_format(
                            self.cur_row,
                            col_cnt,
                            total.count as f64,
                            &self.styles.int_count,
                        )
                        .map_err(|e| e.to_string())?;
                }
            }
            self.cur_row += 1;
        }
        let end_row = self.cur_row - 1;
        if end_row > start_row {
            self.ws
                .merge_range(start_row, 0, end_row, 0, category, &self.styles.text_center)
                .map_err(|e| e.to_string())?;
        }
        Ok(self)
    }

    pub fn write_row(
        &mut self,
        row: &[Data],
        columns: &[ReportColumn],
    ) -> Result<&mut Self, String> {
        write_report_row(self.ws, self.styles, self.cur_row, row, columns)?;
        self.cur_row += 1;
        Ok(self)
    }

    pub fn write_failed_upload_row(
        &mut self,
        record: &FailedUploadRecord<'_>,
        columns: &[ReportColumn],
    ) -> Result<&mut Self, String> {
        self.ws
            .set_row_height(self.cur_row, 22.0)
            .map_err(|e| e.to_string())?;
        for (column, target) in columns.iter().zip([0, 1, 3, 4, 5, 6, 7]) {
            if target == 3 {
                self.ws
                    .write_string_with_format(self.cur_row, 2, "审核失败", &self.styles.text_left)
                    .map_err(|e| e.to_string())?;
            }
            if target == 5 {
                let amount =
                    cell_to_decimal(column.cell(record.row)).unwrap_or(rust_decimal::Decimal::ZERO);
                self.ws
                    .write_with_format(self.cur_row, target, amount, &self.styles.money)
                    .map_err(|e| e.to_string())?;
            } else {
                column.kind.write(
                    self.ws,
                    self.styles,
                    self.cur_row,
                    target,
                    column.cell(record.row),
                )?;
            }
        }
        match &record.extra {
            FailedUploadExtra::ApplianceProductName(prod_name) => {
                self.ws
                    .write_string_with_format(self.cur_row, 8, prod_name, &self.styles.text_left)
                    .map_err(|e| e.to_string())?;
            }
            FailedUploadExtra::DigitalImei { imei1, imei2 } => {
                self.ws
                    .write_string_with_format(self.cur_row, 8, imei1, &self.styles.text_left)
                    .map_err(|e| e.to_string())?;
                self.ws
                    .write_string_with_format(self.cur_row, 9, imei2, &self.styles.text_left)
                    .map_err(|e| e.to_string())?;
            }
        }
        self.cur_row += 1;
        Ok(self)
    }

    fn add_unique_reference_highlight(
        &mut self,
        start_row: u32,
        end_row: u32,
        ref_col: u16,
        max_col: u16,
    ) -> Result<&mut Self, String> {
        if start_row <= end_row {
            let ref_col_name = column_number_to_name(ref_col);
            let first_row = start_row + 1;
            let last_row = end_row + 1;
            let rule = format!(
                "=AND(${ref_col_name}{first_row}<>\"\",COUNTIF(${ref_col_name}${first_row}:${ref_col_name}${last_row},${ref_col_name}{first_row})=1)"
            );
            let highlight = ConditionalFormatFormula::new()
                .set_rule(rule.as_str())
                .set_format(Format::new().set_background_color(Color::RGB(0xFFF2CC)));
            self.ws
                .add_conditional_format(start_row, 0, end_row, max_col, &highlight)
                .map_err(|e| e.to_string())?;
        }
        Ok(self)
    }

    pub fn write_rows_with_unique_reference_highlight(
        &mut self,
        rows: &[&[Data]],
        columns: &[ReportColumn],
        ref_col: u16,
        max_col: u16,
    ) -> Result<&mut Self, String> {
        let start_row = self.cur_row;
        for row in rows {
            self.write_row(row, columns)?;
        }
        if start_row < self.cur_row {
            self.add_unique_reference_highlight(start_row, self.cur_row - 1, ref_col, max_col)?;
        }
        Ok(self)
    }

    pub fn write_final_match_row(
        &mut self,
        seq: usize,
        store_row: &[Data],
        store_cols: &[ReportColumn],
        rec: &ReconciledTransaction<'_>,
    ) -> Result<&mut Self, String> {
        self.ws
            .set_row_height(self.cur_row, 22.0)
            .map_err(|e| e.to_string())?;
        self.ws
            .write_number_with_format(self.cur_row, 0, seq as f64, &self.styles.int_count)
            .map_err(|e| e.to_string())?;

        for (index, column) in store_cols.iter().enumerate() {
            column.kind.write(
                self.ws,
                self.styles,
                self.cur_row,
                index as u16 + 1,
                column.cell(store_row),
            )?;
        }

        self.ws
            .write_string_with_format(self.cur_row, 21, rec.category(), &self.styles.text_center)
            .map_err(|e| e.to_string())?;
        self.ws
            .write_string_with_format(self.cur_row, 22, rec.brand(), &self.styles.text_left)
            .map_err(|e| e.to_string())?;
        self.ws
            .write_string_with_format(self.cur_row, 23, rec.model(), &self.styles.text_left)
            .map_err(|e| e.to_string())?;
        self.ws
            .write_string_with_format(self.cur_row, 24, rec.status, &self.styles.text_center)
            .map_err(|e| e.to_string())?;
        self.ws
            .write_string_with_format(self.cur_row, 25, rec.invoice_no, &self.styles.text_left)
            .map_err(|e| e.to_string())?;
        self.ws
            .write_string_with_format(
                self.cur_row,
                26,
                rec.red_flush_text(),
                &self.styles.text_center,
            )
            .map_err(|e| e.to_string())?;

        self.cur_row += 1;
        Ok(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn incomplete_date_text_stays_text_through_report_xlsx_roundtrip() {
        use calamine::{Reader, open_workbook_auto};

        let path =
            crate::test_support::unique_temp_path("report-invalid-date").with_extension("xlsx");
        let mut book = Workbook::new();
        let styles = StylePool::default();
        let sheet = book.add_worksheet();
        // 报表第6.3节：缺少日期的时间和其他无法识别的日期不得变成数值单元格。
        let values = ["17:19", "17:19:00", "2 中文日期", "2026-03-190", "不是日期"];
        for (row, text) in values.iter().enumerate() {
            for (col, kind) in [CellKind::Date, CellKind::DateTime].into_iter().enumerate() {
                kind.write(
                    sheet,
                    &styles,
                    row as u32,
                    col as u16,
                    &Data::String((*text).into()),
                )
                .unwrap();
            }
        }
        book.save(&path).unwrap();
        let mut result = open_workbook_auto(&path).unwrap();
        let range = result.worksheet_range_at(0).unwrap().unwrap();
        for (row, text) in values.iter().enumerate() {
            for col in 0..2 {
                assert_eq!(
                    range.get_value((row as u32, col)),
                    Some(&Data::String((*text).into()))
                );
            }
        }
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn compact_date_keeps_calendar_date_through_xlsx_roundtrip() {
        use calamine::{Reader, open_workbook_auto};

        let path = crate::test_support::unique_temp_path("report-date").with_extension("xlsx");
        let mut book = Workbook::new();
        let sheet = book.add_worksheet();
        let format = Format::new().set_num_format("yyyy-mm-dd hh:mm:ss");
        // The generic XLSX string parser previously interpreted this source as time only.
        for (row, text) in [
            "20260303 17:19",
            "2026年06月15日",
            "2 2026年04月24日",
            "不是有效日期",
        ]
        .iter()
        .enumerate()
        {
            write_date_cell(sheet, row as u32, 0, &Data::String((*text).into()), &format).unwrap();
        }
        book.save(&path).unwrap();
        let mut result = open_workbook_auto(&path).unwrap();
        let range = result.worksheet_range_at(0).unwrap().unwrap();
        assert_eq!(
            range.get_value((0, 0)).unwrap().as_datetime(),
            chrono::NaiveDate::from_ymd_opt(2026, 3, 3)
                .unwrap()
                .and_hms_opt(17, 19, 0)
        );
        assert_eq!(
            range.get_value((1, 0)).unwrap().as_date(),
            chrono::NaiveDate::from_ymd_opt(2026, 6, 15)
        );
        assert_eq!(
            range.get_value((2, 0)).unwrap().as_date(),
            chrono::NaiveDate::from_ymd_opt(2026, 4, 24)
        );
        assert_eq!(
            range.get_value((3, 0)),
            Some(&Data::String("不是有效日期".into()))
        );
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn report_sheet_builder_initializes_and_tracks_row_flow() {
        let mut wb = Workbook::new();
        let styles = StylePool::default();
        let mut builder = ReportSheetBuilder::new(&mut wb, "测试表", &styles).unwrap();
        assert_eq!(builder.cur_row(), 0);

        builder.set_column_widths(&[10.0, 20.0]).unwrap();
        builder
            .write_title_block(1, "主标题", "副标题说明文字")
            .unwrap();
        assert_eq!(builder.cur_row(), 3);

        builder.write_section_header(1, "第一节").unwrap();
        assert_eq!(builder.cur_row(), 4);

        builder.write_headers(&["列1", "列2"]).unwrap();
        assert_eq!(builder.cur_row(), 5);

        let row_data = [Data::String("值1".into()), Data::String("值2".into())];
        let cols = [
            ReportColumn::new(Some(0), "列1"),
            ReportColumn::new(Some(1), "列2"),
        ];
        builder.write_row(&row_data, &cols).unwrap();
        assert_eq!(builder.cur_row(), 6);

        builder.write_blank_row(8.0).unwrap();
        assert_eq!(builder.cur_row(), 7);

        // Test grouped column headers
        builder
            .write_grouped_column_headers(&[
                ColumnGroup {
                    title: "分组1",
                    subheaders: &["子列1", "子列2"],
                },
                ColumnGroup {
                    title: "独立列",
                    subheaders: &[],
                },
            ])
            .unwrap();
        assert_eq!(builder.cur_row(), 9);

        // Test tip row and notes
        builder.write_tip_row(2, "提示行说明").unwrap();
        assert_eq!(builder.cur_row(), 10);
        builder.write_notes(2, &["说明条目1", "说明条目2"]).unwrap();
        assert_eq!(builder.cur_row(), 12);
    }
}
