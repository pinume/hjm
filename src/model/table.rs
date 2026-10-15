use super::error::ProcessError;
use super::schema::Column;
use super::style::Fill;
use super::value::Value;

/// 行数据。
#[derive(Debug)]
pub struct Row {
    pub values: Vec<Value>,
    pub fill: Option<Fill>,
}

/// 整张工作表数据。
///
/// 注意：为杜绝万级数据行在管线中发生内存克隆，`Table` 刻意不实现 `Clone`。
/// 各任务需以所有权移交（如 `take_uploaded_table`）或只读借用（`&Table`）方式传递。
#[derive(Debug)]
pub struct Table {
    pub columns: Vec<Column>,
    pub rows: Vec<Row>,
}

impl Table {
    /// 写出前的最后一道保险：确认每行的值数量与列数一致。
    pub fn validate(&self) -> Result<(), ProcessError> {
        for (index, row) in self.rows.iter().enumerate() {
            if row.values.len() != self.columns.len() {
                return Err(ProcessError::Structure {
                    file: String::new(),
                    sheet: String::new(),
                    detail: format!(
                        "第{}行的值数量（{}）与列数（{}）不一致",
                        index + 1,
                        row.values.len(),
                        self.columns.len()
                    ),
                });
            }
        }
        Ok(())
    }
}
