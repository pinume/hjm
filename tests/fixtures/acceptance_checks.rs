//! Independent dataset checks derived from the cleaning and reporting business rules.
//! This module never calls cleaning jobs, reconcilers, or report metric calculators.
use calamine::{Data, DataType, Reader, open_workbook_auto};
use chrono::NaiveDate;
use quick_xml::{Reader as XmlReader, events::Event};
use rust_decimal::Decimal;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use subsidy::utils::merchant::{APPLIANCE_COMPUTER_MERCHANT_NO, DIGITAL_MERCHANT_NO};

const PINK: &str = "FFFFC7CE";
const YELLOW: &str = "FFFFEB9C";
const STATUSES: [&str; 4] = ["已回款", "审核通过未回款", "待审核", "审核失败"];

struct Table {
    header: Vec<String>,
    rows: Vec<Vec<Data>>,
}

impl Table {
    fn read(path: &Path, sheet: usize, anchor: &str, footer: bool) -> Self {
        let mut book = open_workbook_auto(path).unwrap();
        let range = book.worksheet_range_at(sheet).unwrap().unwrap();
        Self::from_range(path, &range, anchor, footer)
    }

    fn from_range(path: &Path, range: &calamine::Range<Data>, anchor: &str, footer: bool) -> Self {
        let mut rows: Vec<_> = range.rows().map(|r| r.to_vec()).collect();
        let header = rows
            .iter()
            .position(|r| r.iter().any(|c| c == anchor))
            .unwrap_or_else(|| panic!("{}: missing header {anchor}", path.display()));
        let mut data = rows.split_off(header + 1);
        if footer {
            assert!(data.pop().is_some(), "{}: missing footer", path.display());
        }
        Self {
            header: rows[header].iter().map(ToString::to_string).collect(),
            rows: data,
        }
    }

    fn cell<'a>(&self, row: &'a [Data], aliases: &[&str]) -> &'a Data {
        aliases
            .iter()
            .find_map(|name| self.header.iter().position(|h| h == name))
            .and_then(|col| row.get(col))
            .unwrap_or(&Data::Empty)
    }

    fn text(&self, row: &[Data], aliases: &[&str]) -> String {
        self.cell(row, aliases).to_string()
    }

    fn project(&self, row: &[Data], fields: &[&str]) -> Vec<String> {
        fields
            .iter()
            .map(|field| self.text(row, &[*field]))
            .collect()
    }
}

fn files(input: &Path, predicate: impl Fn(&str) -> bool) -> Vec<PathBuf> {
    let mut paths: Vec<_> = std::fs::read_dir(input)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| {
            let name = p.file_name().unwrap().to_str().unwrap();
            p.is_file() && !name.starts_with("~$") && predicate(name)
        })
        .collect();
    paths.sort();
    assert!(!paths.is_empty(), "no source files matched");
    paths
}

fn raw_tables(paths: &[PathBuf], anchor: &str, footer: bool) -> Vec<Table> {
    paths
        .iter()
        .flat_map(|path| {
            let mut book = open_workbook_auto(path).unwrap();
            book.sheet_names()
                .into_iter()
                .enumerate()
                .filter(|(_, name)| name != "汇总")
                .map(|(index, _)| {
                    let range = book.worksheet_range_at(index).unwrap().unwrap();
                    Table::from_range(path, &range, anchor, footer)
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

fn money(cell: &Data) -> Decimal {
    if matches!(cell, Data::Empty) {
        Decimal::ZERO
    } else {
        Decimal::from_str(&cell.to_string()).unwrap().round_dp(2)
    }
}

fn close(actual: Decimal, expected: Decimal) {
    assert!(
        (actual - expected).abs() <= Decimal::new(1, 5),
        "actual {actual}, expected {expected}"
    );
}

fn multiset(mut actual: Vec<Vec<String>>, mut expected: Vec<Vec<String>>, label: &str) {
    actual.sort();
    expected.sort();
    ordered(actual, expected, label);
}

fn ordered(actual: Vec<Vec<String>>, expected: Vec<Vec<String>>, label: &str) {
    assert_eq!(actual.len(), expected.len(), "{label}: detail count");
    for (index, (actual, expected)) in actual.iter().zip(&expected).enumerate() {
        assert_eq!(actual, expected, "{label}: detail {}", index + 1);
    }
}

fn match_no(date: &Data, doc: &str) -> String {
    if matches!(date, Data::Empty) || doc.is_empty() {
        return String::new();
    }
    let text = date.to_string();
    let day = date
        .as_date()
        .or_else(|| {
            ["%Y-%m-%d", "%Y/%m/%d", "%Y%m%d"]
                .iter()
                .find_map(|format| {
                    NaiveDate::parse_from_str(text.split(' ').next().unwrap(), format).ok()
                })
        })
        .unwrap_or_else(|| panic!("unrecognized source date: {text}"));
    format!(
        "{}{}",
        day.format("%y%m%d"),
        doc.strip_prefix("收款").unwrap_or(doc)
    )
}

pub fn check(input: &Path, cleaned: &Path, summary: &Path, finance: &Path) {
    let invoice_paths = files(input, |n| n.starts_with("发票_") && n.ends_with(".xlsx"));
    let invoices = Table::read(invoice_paths.last().unwrap(), 0, "主要商品名称", false);
    let invoice_out = Table::read(&cleaned.join("发票明细.xlsx"), 0, "主要商品名称", false);
    assert_eq!(invoice_out.header.len(), 10);
    check_invoices(&invoices, &invoice_out, &cleaned.join("发票明细.xlsx"));

    let mut uploads = Vec::new();
    let mut refunds = Vec::new();
    let mut refund_pink = Vec::new();
    for (merchant, upload_name, suffix, refund_name) in [
        (
            APPLIANCE_COMPUTER_MERCHANT_NO,
            "已上传家电电脑.xlsx",
            "年以旧换新补贴明细.xlsx",
            "回款明细家电电脑.xlsx",
        ),
        (
            DIGITAL_MERCHANT_NO,
            "已上传数码.xlsx",
            "年数码补贴明细.xlsx",
            "回款明细数码.xlsx",
        ),
    ] {
        let paths = files(input, |n| {
            n.starts_with(&format!("MER_{merchant}_")) && n.ends_with(".xlsx")
        });
        let source = raw_tables(&paths, "实时清分UUID", false);
        let out = Table::read(&cleaned.join(upload_name), 0, "实时清分UUID", false);
        assert_eq!(out.header.len(), 59);
        multiset(
            out.rows.iter().map(|r| upload_record(&out, r)).collect(),
            source
                .iter()
                .flat_map(|t| t.rows.iter().map(|r| upload_record(t, r)))
                .collect(),
            upload_name,
        );
        let terminal: Vec<_> = out
            .rows
            .iter()
            .map(|r| out.text(r, &["状态"]) == "审核终止")
            .collect();
        assert!(
            terminal.windows(2).all(|p| !p[0] || p[1]),
            "未匹配审核终止未沉底"
        );
        let upload_colors: Vec<_> = out.rows.iter().map(|_| None).collect();
        check_colors(&cleaned.join(upload_name), &upload_colors, 59);
        uploads.push(out);

        let paths = files(input, |n| n.ends_with(suffix));
        let source = raw_tables(&[paths.last().unwrap().clone()], "拨付批次", false);
        let out = Table::read(&cleaned.join(refund_name), 0, "拨付批次", false);
        assert_eq!(out.header.len(), 24);
        refund_pink.push(check_refunds(
            &source,
            &out,
            merchant,
            &cleaned.join(refund_name),
        ));
        refunds.push(out);
    }

    let receipts = Table::read(&input.join("收款单统计.xlsx"), 0, "单据号", true);
    let receipts_out = Table::read(&cleaned.join("收款单统计.xlsx"), 0, "单据号", false);
    assert_eq!(receipts_out.header.len(), 6);
    let fields = ["单据号", "商品名称", "原票号"];
    assert_eq!(
        receipts_out
            .rows
            .iter()
            .map(|r| receipts_out.project(r, &fields))
            .collect::<Vec<_>>(),
        receipts
            .rows
            .iter()
            .map(|r| receipts.project(r, &fields))
            .collect::<Vec<_>>()
    );
    for (raw, out) in receipts.rows.iter().zip(&receipts_out.rows) {
        assert_eq!(
            receipts_out.text(out, &["匹配单据号"]),
            match_no(
                receipts.cell(raw, &["日期"]),
                &receipts.text(raw, &["单据号"])
            )
        );
    }
    check_colors(
        &cleaned.join("收款单统计.xlsx"),
        &vec![None; receipts_out.rows.len()],
        6,
    );

    let sales = Table::read(&input.join("销售用券情况统计.xlsx"), 0, "单据号", true);
    let sales_out = Table::read(&cleaned.join("销售用券情况统计.xlsx"), 0, "单据号", false);
    assert_eq!(sales_out.header.len(), 11);
    check_sales(
        &sales,
        &sales_out,
        &receipts_out,
        &cleaned.join("销售用券情况统计.xlsx"),
    );

    let union_paths = files(input, |n| {
        n.ends_with(".xlsx")
            && [APPLIANCE_COMPUTER_MERCHANT_NO, DIGITAL_MERCHANT_NO]
                .iter()
                .any(|m| n.starts_with(&format!("{m}_MX_")))
    });
    let union_source = raw_tables(&union_paths, "清算时间", true);
    let union = Table::read(&cleaned.join("银联交易明细门店.xlsx"), 0, "清算时间", false);
    assert_eq!(union.header.len(), 26);
    check_union(
        &union_source,
        &union,
        &cleaned.join("银联交易明细门店.xlsx"),
    );

    check_summary(summary, &sales, &uploads);
    check_anomalies(summary, &uploads, &refunds, &refund_pink, &invoice_out);
    for (sheet, count, width) in [
        (0, union.rows.len(), 27),
        (1, union.rows.len(), 26),
        (2, refunds.iter().map(|t| t.rows.len()).sum(), 24),
        (3, uploads.iter().map(|t| t.rows.len()).sum(), 60),
    ] {
        let mut book = open_workbook_auto(finance).unwrap();
        let range = book.worksheet_range_at(sheet).unwrap().unwrap();
        assert_eq!(range.height(), count + 1, "财务统筹sheet {sheet}遗漏明细");
        assert_eq!(range.width(), width);
    }
}

fn money_text(cell: &Data) -> String {
    if cell.to_string().is_empty() {
        String::new()
    } else {
        money(cell).normalize().to_string()
    }
}

fn upload_record(table: &Table, row: &[Data]) -> Vec<String> {
    let mut record = table.project(row, &["实时清分UUID", "订单号", "检索参考号"]);
    record.push(money_text(table.cell(row, &["交易金额"])));
    record
}

fn union_record(table: &Table, row: &[Data]) -> Vec<String> {
    let mut record = table.project(row, &["检索号", "商户订单号", "交易类型"]);
    record.push(money_text(table.cell(row, &["交易金额"])));
    record.push(money_text(table.cell(row, &["优惠金额"])));
    record.push(table.text(row, &["备注"]));
    record
}

fn check_union(raw: &[Table], out: &Table, path: &Path) {
    // §6.4: every refund sinks; only a unique original consumption is also marked.
    let records: Vec<_> = raw
        .iter()
        .flat_map(|t| t.rows.iter().map(|r| union_record(t, r)))
        .collect();
    let is_return = |kind: &str| ["消费撤销", "消费撤消", "联机退货"].contains(&kind);
    let mut consumers = HashMap::new();
    let mut returned = HashSet::new();
    for row in &records {
        if !row[0].is_empty() {
            if row[2] == "消费" {
                *consumers.entry(&row[0]).or_insert(0) += 1;
            }
            if is_return(&row[2]) {
                returned.insert(&row[0]);
            }
        }
    }
    let mut groups: [Vec<Vec<String>>; 2] = std::array::from_fn(|_| Vec::new());
    for row in &records {
        let pink = is_return(&row[2])
            || (row[2] == "消费"
                && returned.contains(&row[0])
                && consumers.get(&row[0]).copied().unwrap_or(0) == 1);
        let mut record = row.clone();
        if pink {
            record[5] = "已退货".to_string();
        }
        groups[usize::from(pink)].push(record);
    }
    let colors: Vec<_> = groups
        .iter()
        .enumerate()
        .flat_map(|(i, rows)| rows.iter().map(move |_| (i == 1).then_some(PINK)))
        .collect();
    ordered(
        out.rows
            .iter()
            .map(|r| union_record(out, r))
            .collect::<Vec<_>>(),
        groups.into_iter().flatten().collect::<Vec<_>>(),
        "银联明细、金额、退货标记或稳定分区错误",
    );
    check_colors(path, &colors, 26);
}

fn invoice_record(table: &Table, row: &[Data]) -> Vec<String> {
    let mut record = table.project(
        row,
        &[
            "开票类型",
            "数电发票号码",
            "购方名称",
            "主要商品名称",
            "备注信息",
            "开票状态",
        ],
    );
    record[3] = record[3].rsplit('*').next().unwrap().to_string();
    record
}

fn check_invoices(raw: &Table, out: &Table, path: &Path) {
    let keys: HashMap<_, _> = out
        .rows
        .iter()
        .map(|r| (invoice_record(out, r), out.text(r, &["匹配单据号"])))
        .collect();
    let abnormal = |t: &Table, r: &[Data]| {
        t.text(r, &["开票类型"]) == "红票"
            || ["已红冲", "开票失败"].contains(&t.text(r, &["开票状态"]).as_str())
    };
    let mut counts = HashMap::new();
    for row in &raw.rows {
        if !abnormal(raw, row) {
            let key = keys.get(&invoice_record(raw, row)).expect("发票源明细缺失");
            if !key.is_empty() {
                *counts.entry(key).or_insert(0) += 1;
            }
        }
    }
    let mut groups: [Vec<Vec<String>>; 3] = std::array::from_fn(|_| Vec::new());
    for row in &raw.rows {
        let record = invoice_record(raw, row);
        let key = keys.get(&record).expect("发票源明细缺失");
        let group = if abnormal(raw, row) {
            2
        } else if counts.get(key).copied().unwrap_or(0) > 1 {
            1
        } else {
            0
        };
        groups[group].push(record);
    }
    let colors: Vec<_> = groups
        .iter()
        .enumerate()
        .flat_map(|(g, rows)| {
            rows.iter()
                .map(move |_| [None, Some(YELLOW), Some(PINK)][g])
        })
        .collect();
    ordered(
        out.rows
            .iter()
            .map(|r| invoice_record(out, r))
            .collect::<Vec<_>>(),
        groups.into_iter().flatten().collect::<Vec<_>>(),
        "发票明细或稳定分区错误",
    );
    check_colors(path, &colors, 10);
}

fn refund_record(table: &Table, row: &[Data]) -> Vec<String> {
    vec![
        table.text(row, &["商户订单号"]),
        table.text(row, &["拨付批次"]),
        table.text(row, &["交易参考号", "参考号"]),
        money(table.cell(row, &["补贴金额", "补贴销售金额"]))
            .normalize()
            .to_string(),
    ]
}

fn check_refunds(raw: &[Table], out: &Table, merchant: &str, path: &Path) -> Vec<bool> {
    let records: Vec<_> = raw
        .iter()
        .flat_map(|t| {
            t.rows
                .iter()
                .filter(|r| {
                    t.text(r, &["核销商编", "经销商编号", "商户编号", "商户号"]) == merchant
                })
                .map(|r| refund_record(t, r))
        })
        .collect();
    let mut groups: HashMap<&str, (usize, Decimal, usize)> = HashMap::new();
    for (index, row) in records.iter().enumerate() {
        let group = groups.entry(&row[0]).or_default();
        group.0 += 1;
        group.1 += Decimal::from_str(&row[3]).unwrap();
        group.2 = index;
    }
    let mut partition: [Vec<Vec<String>>; 2] = std::array::from_fn(|_| Vec::new());
    for (index, record) in records.iter().enumerate() {
        let (count, amount, last) = groups[record[0].as_str()];
        let pink = if count == 1 {
            amount < Decimal::ZERO
        } else {
            amount <= Decimal::ZERO || index != last
        };
        partition[usize::from(pink)].push(record.clone());
    }
    let flags: Vec<_> = partition
        .iter()
        .enumerate()
        .flat_map(|(i, rows)| rows.iter().map(move |_| i == 1))
        .collect();
    ordered(
        out.rows
            .iter()
            .map(|r| refund_record(out, r))
            .collect::<Vec<_>>(),
        partition.into_iter().flatten().collect::<Vec<_>>(),
        &format!("{merchant}:回款筛选、金额或稳定分区错误"),
    );
    check_colors(
        path,
        &flags
            .iter()
            .map(|pink| pink.then_some(PINK))
            .collect::<Vec<_>>(),
        24,
    );
    flags
}

fn check_sales(raw: &Table, out: &Table, receipts: &Table, path: &Path) {
    let mut remarks: HashMap<String, HashSet<String>> = HashMap::new();
    for row in &receipts.rows {
        let remark = receipts.text(row, &["备注"]);
        if !remark.is_empty() {
            remarks
                .entry(receipts.text(row, &["匹配单据号"]))
                .or_default()
                .insert(remark);
        }
    }
    let mut partition: [Vec<Vec<String>>; 2] = std::array::from_fn(|_| Vec::new());
    for row in &raw.rows {
        let doc = raw.text(row, &["单据号"]);
        let key = match_no(raw.cell(row, &["单据日期"]), &doc);
        let pink = !key.is_empty() && remarks.get(&key).is_some_and(|r| r.len() == 1);
        partition[usize::from(pink)].push(vec![
            doc,
            raw.text(row, &["商品名称"]),
            money(raw.cell(row, &["合计"])).normalize().to_string(),
            key,
        ]);
    }
    let colors: Vec<_> = partition
        .iter()
        .enumerate()
        .flat_map(|(i, rows)| rows.iter().map(move |_| (i == 1).then_some(PINK)))
        .collect();
    let actual: Vec<_> = out
        .rows
        .iter()
        .map(|row| {
            let amount = money(out.cell(row, &["补贴额"]));
            let quantity = if amount > Decimal::ZERO {
                1
            } else if amount < Decimal::ZERO {
                -1
            } else {
                0
            };
            assert_eq!(money(out.cell(row, &["数量"])), Decimal::from(quantity));
            let key = out.text(row, &["匹配单据号"]);
            if let Some(values) = remarks.get(&key).filter(|v| v.len() == 1) {
                assert!(values.contains(&out.text(row, &["备注"])));
            }
            vec![
                out.text(row, &["单据号"]),
                out.text(row, &["商品名称"]),
                amount.normalize().to_string(),
                key,
            ]
        })
        .collect();
    ordered(
        actual,
        partition.into_iter().flatten().collect::<Vec<_>>(),
        "用券金额、匹配单据号或稳定分区错误",
    );
    check_colors(path, &colors, 11);
}

fn number(cell: &Data) -> Decimal {
    if matches!(cell, Data::Empty) {
        Decimal::ZERO
    } else {
        Decimal::from_str(&cell.to_string()).unwrap()
    }
}

fn check_summary(path: &Path, raw_sales: &Table, uploads: &[Table]) {
    // Reporting rules §4.1: source sales supply net occurrence; uploaded details supply status totals.
    let mut metrics = [[(Decimal::ZERO, 0_i64); 3]; 7];
    for row in &raw_sales.rows {
        let digital = ["数码", "新业务类"].contains(&raw_sales.text(row, &["财务大类"]).as_str());
        let amount = money(raw_sales.cell(row, &["合计"]));
        let entry = &mut metrics[0][usize::from(digital)];
        entry.0 += amount;
        entry.1 += if amount > Decimal::ZERO {
            1
        } else if amount < Decimal::ZERO {
            -1
        } else {
            0
        };
    }
    for (group, table) in uploads.iter().enumerate() {
        for row in &table.rows {
            if let Some(status) = STATUSES
                .iter()
                .position(|s| *s == table.text(row, &["状态"]))
            {
                let entry = &mut metrics[[1, 3, 4, 5][status]][group];
                entry.0 += money(table.cell(row, &["补贴金额"]));
                entry.1 += 1;
            }
        }
    }
    for group in [0, 1] {
        metrics[2][group] = (
            metrics[0][group].0 - metrics[1][group].0,
            metrics[0][group].1 - metrics[1][group].1,
        );
        metrics[6][group] = (
            metrics[2][group].0 - (3..6).map(|i| metrics[i][group].0).sum::<Decimal>(),
            metrics[2][group].1 - (3..6).map(|i| metrics[i][group].1).sum::<i64>(),
        );
    }
    for row in &mut metrics {
        row[2] = (row[0].0 + row[1].0, row[0].1 + row[1].1);
    }
    let mut book = open_workbook_auto(path).unwrap();
    let range = book.worksheet_range_at(0).unwrap().unwrap();
    let positions = [6, 7, 8, 10, 11, 12, 13];
    for (i, row_index) in positions.iter().enumerate() {
        for (group, (amount, count)) in metrics[i].iter().enumerate() {
            close(
                number(range.get((*row_index, 1 + group * 2)).unwrap()) * Decimal::from(10000),
                *amount,
            );
            assert_eq!(
                number(range.get((*row_index, 2 + group * 2)).unwrap()),
                Decimal::from(*count)
            );
        }
    }
    let positions = [18, 19, 20, 21, 23, 24, 25, 26];
    for (i, row_index) in positions.iter().enumerate() {
        for group in 0..3 {
            let amount = match i {
                0 => metrics[0][group].0,
                1 => metrics[1][group].0,
                2 => metrics[1][group].0 + metrics[3][group].0,
                3 => metrics[2][group].0,
                _ => metrics[i - 1][group].0,
            };
            let denominator = if i == 0 {
                metrics[0][2].0
            } else if i < 4 {
                metrics[0][group].0
            } else {
                metrics[2][group].0
            };
            let ratio = if denominator.is_zero() {
                Decimal::ZERO
            } else {
                amount / denominator
            };
            close(
                number(range.get((*row_index, 1 + group * 2)).unwrap()) * Decimal::from(10000),
                amount,
            );
            let actual = number(range.get((*row_index, 2 + group * 2)).unwrap());
            assert!(
                (actual - ratio).abs() < Decimal::new(1, 10),
                "汇总比率row {row_index}, group {group}: {actual} != {ratio}"
            );
        }
    }
}

fn check_anomalies(
    path: &Path,
    uploads: &[Table],
    refunds: &[Table],
    pink: &[Vec<bool>],
    invoices: &Table,
) {
    let mut book = open_workbook_auto(path).unwrap();
    let failed = book.worksheet_range_at(2).unwrap().unwrap();
    let expected: Vec<_> = uploads
        .iter()
        .flat_map(|t| {
            t.rows
                .iter()
                .filter(|r| t.text(r, &["状态"]) == "审核失败")
                .map(|r| t.project(r, &["检索参考号", "发票号码"]))
        })
        .collect();
    let actual: Vec<_> = failed
        .rows()
        .filter(|r| r.get(2).is_some_and(|v| v == "审核失败"))
        .map(|r| vec![r[1].to_string(), r[4].to_string()])
        .collect();
    multiset(actual, expected, "审核失败明细");
    let digital_section = failed
        .rows()
        .position(|r| r[0] == "数码审核失败明细")
        .unwrap();
    let names: Vec<_> = failed
        .rows()
        .take(digital_section)
        .filter(|r| r.get(2).is_some_and(|v| v == "审核失败"))
        .map(|r| r[8].to_string())
        .collect();
    assert!(
        names
            .windows(2)
            .all(|r| (r[0].is_empty(), &r[0]) <= (r[1].is_empty(), &r[1])),
        "家电审核失败商品名未排序"
    );

    let anomalies = Table::read(path, 3, "拨付批次", false);
    for (group, merchant) in [APPLIANCE_COMPUTER_MERCHANT_NO, DIGITAL_MERCHANT_NO]
        .iter()
        .enumerate()
    {
        let expected: Vec<_> = refunds[group]
            .rows
            .iter()
            .zip(&pink[group])
            .filter(|(_, is_pink)| **is_pink)
            .map(|(r, _)| refund_record(&refunds[group], r))
            .collect();
        let actual: Vec<_> = anomalies
            .rows
            .iter()
            .filter(|r| anomalies.text(r, &["核销商编"]) == *merchant)
            .map(|r| refund_record(&anomalies, r))
            .collect();
        multiset(actual.clone(), expected, "异常回款明细");
        let references: Vec<_> = actual.iter().map(|r| &r[2]).collect();
        let mut counts = HashMap::new();
        for reference in &references {
            *counts.entry(*reference).or_insert(0) += 1;
        }
        let sort_keys: Vec<_> = references
            .iter()
            .map(|r| (r.is_empty(), counts[r] == 1, *r))
            .collect();
        assert!(
            sort_keys.windows(2).all(|r| r[0] <= r[1]),
            "异常回款参考号未按重复/唯一和文本排序"
        );
    }
    let mut counts = HashMap::new();
    for row in &invoices.rows {
        let key = invoices.text(row, &["匹配单据号"]);
        let abnormal = invoices.text(row, &["开票类型"]) == "红票"
            || ["已红冲", "开票失败"].contains(&invoices.text(row, &["开票状态"]).as_str());
        if !abnormal && !key.is_empty() {
            *counts.entry(key).or_insert(0) += 1;
        }
    }
    let fields = ["数电发票号码", "主要商品名称", "备注信息", "匹配单据号"];
    let expected: Vec<_> = invoices
        .rows
        .iter()
        .filter(|r| {
            let abnormal = invoices.text(r, &["开票类型"]) == "红票"
                || ["已红冲", "开票失败"].contains(&invoices.text(r, &["开票状态"]).as_str());
            !abnormal
                && counts
                    .get(&invoices.text(r, &["匹配单据号"]))
                    .copied()
                    .unwrap_or(0)
                    > 1
        })
        .map(|r| invoices.project(r, &fields))
        .collect();
    let anomalous_invoices = Table::read(path, 4, "开票时间", false);
    multiset(
        anomalous_invoices
            .rows
            .iter()
            .map(|r| anomalous_invoices.project(r, &fields))
            .collect(),
        expected,
        "异常发票明细",
    );
    let keys: Vec<_> = anomalous_invoices
        .rows
        .iter()
        .map(|r| anomalous_invoices.text(r, &["匹配单据号"]))
        .collect();
    assert!(
        keys.windows(2).all(|r| r[0] <= r[1]),
        "异常发票未按匹配单据号排序"
    );
}

fn attribute(element: &quick_xml::events::BytesStart<'_>, name: &[u8]) -> Option<String> {
    element
        .attributes()
        .map(Result::unwrap)
        .find(|a| a.key.as_ref() == name)
        .map(|a| String::from_utf8(a.value.into_owned()).unwrap())
}

fn xml(path: &Path, entry: &str) -> String {
    let mut archive = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    let mut text = String::new();
    archive
        .by_name(entry)
        .unwrap()
        .read_to_string(&mut text)
        .unwrap();
    text
}

fn check_colors(path: &Path, expected: &[Option<&str>], width: usize) {
    // Read XLSX styles independently; check every written cell of a highlighted row.
    let styles = xml(path, "xl/styles.xml");
    let mut reader = XmlReader::from_str(&styles);
    let mut fills = Vec::new();
    let mut current_fill = None;
    let mut in_fill = false;
    let mut in_xfs = false;
    let mut xfs = Vec::new();
    loop {
        match reader.read_event().unwrap() {
            Event::Start(e) | Event::Empty(e) => match e.name().as_ref() {
                b"fill" => {
                    in_fill = true;
                    current_fill = None;
                }
                b"fgColor" if in_fill => {
                    current_fill = attribute(&e, b"rgb");
                }
                b"cellXfs" => {
                    in_xfs = true;
                }
                b"xf" if in_xfs => {
                    xfs.push(attribute(&e, b"fillId").unwrap().parse::<usize>().unwrap());
                }
                _ => {}
            },
            Event::End(e) => match e.name().as_ref() {
                b"fill" => {
                    fills.push(current_fill.take());
                    in_fill = false;
                }
                b"cellXfs" => {
                    in_xfs = false;
                }
                _ => {}
            },
            Event::Eof => break,
            _ => {}
        }
    }
    let worksheet = xml(path, "xl/worksheets/sheet1.xml");
    let mut reader = XmlReader::from_str(&worksheet);
    let mut colored_cells = BTreeMap::new();
    loop {
        match reader.read_event().unwrap() {
            Event::Start(e) | Event::Empty(e) if e.name().as_ref() == b"c" => {
                let address = attribute(&e, b"r").unwrap();
                let row: usize = address
                    .trim_start_matches(|c: char| c.is_ascii_alphabetic())
                    .parse()
                    .unwrap();
                if row == 1 {
                    continue;
                }
                let expected_color = expected
                    .get(row - 2)
                    .unwrap_or_else(|| panic!("{}: extra output row {row}", path.display()));
                let style = attribute(&e, b"s").map_or(0, |s| s.parse::<usize>().unwrap());
                let color = fills[xfs[style]].as_deref();
                if expected_color.is_some() {
                    assert_eq!(
                        color,
                        *expected_color,
                        "{}: fill at {address}",
                        path.display()
                    );
                    *colored_cells.entry(row).or_insert(0) += 1;
                } else {
                    assert!(
                        color.is_none() || color == Some("FFF3F6FA"),
                        "{}: unexpected highlight at {address}: {color:?}",
                        path.display()
                    );
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    for (index, expected_color) in expected.iter().enumerate() {
        if expected_color.is_some() {
            assert_eq!(
                colored_cells.get(&(index + 2)).copied().unwrap_or(0),
                width,
                "{}: incomplete row fill at {}",
                path.display(),
                index + 2
            );
        }
    }
}
