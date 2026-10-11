use calamine::{Data, DataType};
use rust_xlsxwriter::{ExcelDateTime, Format, Workbook, Worksheet};

use super::data::{CellKind, ReportColumn};
use super::reader::{cell_to_decimal, cell_to_string};
use super::styles::StylePool;

pub fn write_date_cell(
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
    match ExcelDateTime::parse_from_str(&value) {
        Ok(datetime) => ws
            .write_datetime_with_format(row, col, datetime, format)
            .map(|_| ())
            .map_err(|e| e.to_string()),
        Err(error) => {
            eprintln!(
                "[警告] 第 {} 行第 {} 列日期值 '{}' 无法解析，已保留原文本: {}",
                row + 1,
                col + 1,
                value,
                error
            );
            ws.write_string_with_format(row, col, value, format)
                .map(|_| ())
                .map_err(|e| e.to_string())
        }
    }
}

pub fn write_decimal_cell(
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
    pub ws: &'a mut Worksheet,
    pub styles: &'a StylePool,
    pub cur_row: u32,
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

    pub fn write_row(
        &mut self,
        row: &[Data],
        columns: &[ReportColumn],
    ) -> Result<&mut Self, String> {
        write_report_row(self.ws, self.styles, self.cur_row, row, columns)?;
        self.cur_row += 1;
        Ok(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_sheet_builder_initializes_and_tracks_row_flow() {
        let mut wb = Workbook::new();
        let styles = StylePool::default();
        let mut builder = ReportSheetBuilder::new(&mut wb, "测试表", &styles).unwrap();
        assert_eq!(builder.cur_row, 0);

        builder.set_column_widths(&[10.0, 20.0]).unwrap();
        builder
            .write_title_block(1, "主标题", "副标题说明文字")
            .unwrap();
        assert_eq!(builder.cur_row, 3);

        builder.write_section_header(1, "第一节").unwrap();
        assert_eq!(builder.cur_row, 4);

        builder.write_headers(&["列1", "列2"]).unwrap();
        assert_eq!(builder.cur_row, 5);

        let row_data = [Data::String("值1".into()), Data::String("值2".into())];
        let cols = [
            ReportColumn::new(Some(0), "列1"),
            ReportColumn::new(Some(1), "列2"),
        ];
        builder.write_row(&row_data, &cols).unwrap();
        assert_eq!(builder.cur_row, 6);

        builder.write_blank_row(8.0).unwrap();
        assert_eq!(builder.cur_row, 7);
    }
}
