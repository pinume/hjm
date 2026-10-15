use std::fmt;
use std::path::Path;

use calamine::{Data, ExcelDateTime, ExcelDateTimeType, Range, Reader, Xlsx, open_workbook};
use rust_xlsxwriter::IntoExcelDateTime;

use crate::model::ProcessError;

/// 单元格原始值，保留源类型供处理模块判断。
#[derive(Debug, Clone, PartialEq)]
pub enum RawCell {
    Empty,
    Text(String),
    Float(f64),
    Int(i64),
    Bool(bool),
    /// Excel 日期/时间序列值，统一为 1900 日期系统，由`utils::dates`解析。
    DateTime(f64),
    Error(String),
}

impl fmt::Display for RawCell {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RawCell::Empty => Ok(()),
            RawCell::Text(text) => write!(f, "{text}"),
            RawCell::Float(value) => write!(f, "{value}"),
            RawCell::Int(value) => write!(f, "{value}"),
            RawCell::Bool(value) => write!(f, "{value}"),
            RawCell::DateTime(serial) => write!(f, "{serial}"),
            RawCell::Error(error) => write!(f, "{error}"),
        }
    }
}

impl From<&Data> for RawCell {
    fn from(value: &Data) -> Self {
        match value {
            Data::Empty => RawCell::Empty,
            Data::String(text) => RawCell::Text(text.clone()),
            // 部分格式（如 ODS 来源）以 ISO 文本表示日期/时长，按文本交给 utils::dates 解析。
            Data::DateTimeIso(text) | Data::DurationIso(text) => RawCell::Text(text.clone()),
            Data::Float(value) => RawCell::Float(*value),
            Data::Int(value) => RawCell::Int(*value),
            Data::Bool(value) => RawCell::Bool(*value),
            Data::DateTime(value) => {
                let serial = value.as_f64();
                let standard = ExcelDateTime::new(serial, ExcelDateTimeType::DateTime, false);
                // 1900 序列值原样保留；其他纪元复用依赖的日期转换，避免早期日期闰年修正。
                let normalized = match (value.as_datetime(), standard.as_datetime()) {
                    (Some(actual), Some(base)) if actual != base => actual.to_excel_serial_date(),
                    _ => serial,
                };
                RawCell::DateTime(normalized)
            }
            Data::Error(error) => RawCell::Error(error.to_string()),
        }
    }
}

/// 单个工作表的数据区域；行列号均为 Excel 绝对坐标，从 1 开始。
pub struct SheetGrid {
    name: String,
    range: Range<Data>,
}

impl SheetGrid {
    pub fn name(&self) -> &str {
        &self.name
    }

    /// 按 Excel 绝对行列号（从 1 开始）取值；超出数据区域时视为空单元格。
    pub fn cell(&self, row: u32, col: u32) -> RawCell {
        if row == 0 || col == 0 {
            return RawCell::Empty;
        }
        match self.range.get_value((row - 1, col - 1)) {
            Some(data) => RawCell::from(data),
            None => RawCell::Empty,
        }
    }

    /// 最后一个实际有值行的绝对行号（从 1 开始），按实际非空单元格计算。
    pub fn last_value_row(&self) -> Option<u32> {
        let start_row = self.range.start()?.0;
        self.range
            .used_cells()
            .map(|(row, _, _)| row as u32)
            .max()
            .map(|relative_max| start_row + relative_max + 1)
    }

    /// 读取某绝对行在数据区域全部列宽度上的文本值，用于读取表头。
    pub fn row_texts(&self, row: u32) -> Vec<String> {
        let Some((_, start_col)) = self.range.start() else {
            return Vec::new();
        };
        let width = self.range.width() as u32;
        (0..width)
            .map(|offset| match self.cell(row, start_col + offset + 1) {
                RawCell::Text(text) => text,
                RawCell::Int(value) => value.to_string(),
                RawCell::Float(value) => value.to_string(),
                RawCell::Bool(value) => value.to_string(),
                RawCell::Empty | RawCell::DateTime(_) | RawCell::Error(_) => String::new(),
            })
            .collect()
    }
}

/// 打开工作簿并按原顺序返回全部工作表。
pub fn open_sheets(path: &Path) -> Result<Vec<SheetGrid>, ProcessError> {
    open_sheets_matching(path, |_| true)
}

/// 在读取单元格前筛选工作表；所选工作表仍按源顺序读取，读取失败即报错。
pub(crate) fn open_sheets_matching(
    path: &Path,
    include: impl Fn(&str) -> bool,
) -> Result<Vec<SheetGrid>, ProcessError> {
    let mut workbook: Xlsx<_> = open_workbook(path)
        .map_err(|error| ProcessError::Read(format!("{}：{error}", path.display())))?;
    workbook
        .sheet_names()
        .into_iter()
        .filter(|name| include(name))
        .map(|name| {
            let range = workbook.worksheet_range(&name).map_err(|error| {
                ProcessError::Read(format!("{} / {name}：{error}", path.display()))
            })?;
            Ok(SheetGrid { name, range })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn excluded_sheet_is_not_parsed_and_selected_errors_still_propagate() {
        use std::io::{Cursor, Read, Write};

        let mut book = rust_xlsxwriter::Workbook::new();
        book.add_worksheet()
            .set_name("明细")
            .unwrap()
            .write_string(0, 0, "有效")
            .unwrap();
        book.add_worksheet()
            .set_name("汇总")
            .unwrap()
            .write_string(0, 0, "忽略")
            .unwrap();
        let mut archive =
            zip::ZipArchive::new(Cursor::new(book.save_to_buffer().unwrap())).unwrap();
        let mut damaged = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for i in 0..archive.len() {
            let mut entry = archive.by_index(i).unwrap();
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).unwrap();
            if entry.name() == "xl/worksheets/sheet2.xml" {
                bytes = b"<worksheet><sheetData><row></worksheet>".to_vec();
            }
            damaged
                .start_file(entry.name(), zip::write::SimpleFileOptions::default())
                .unwrap();
            damaged.write_all(&bytes).unwrap();
        }
        let path = crate::test_support::unique_temp_path("excluded-sheet").with_extension("xlsx");
        std::fs::write(&path, damaged.finish().unwrap().into_inner()).unwrap();
        let selected = open_sheets_matching(&path, |name| name != "汇总").unwrap();
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].cell(1, 1), RawCell::Text("有效".into()));
        let error = open_sheets(&path)
            .err()
            .expect("selected corrupt sheet must fail");
        assert!(error.to_string().contains("汇总"));
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn standard_epoch_serials_are_unchanged() {
        for serial in [0.5, 59.0, 60.0, 61.0, 45943.123456789] {
            let cell = Data::DateTime(ExcelDateTime::new(
                serial,
                ExcelDateTimeType::DateTime,
                false,
            ));
            assert_eq!(RawCell::from(&cell), RawCell::DateTime(serial));
        }
    }

    #[test]
    fn early_1904_dates_use_the_correct_epoch() {
        for serial in [0.0, 0.5, 59.0, 60.0, 61.0] {
            let cell = Data::DateTime(ExcelDateTime::new(
                serial,
                ExcelDateTimeType::DateTime,
                true,
            ));
            assert_eq!(RawCell::from(&cell), RawCell::DateTime(serial + 1462.0));
        }
    }
}
