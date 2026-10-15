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

#[derive(Clone)]
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
        let upload_source = raw_tables(&paths, "实时清分UUID", false);
        let out = Table::read(&cleaned.join(upload_name), 0, "实时清分UUID", false);
        assert_eq!(out.header.len(), 59);
        multiset(
            out.rows.iter().map(|r| upload_record(&out, r)).collect(),
            upload_source
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
        check_upload_matches(
            &upload_source,
            uploads.last().unwrap(),
            &out,
            refund_pink.last().unwrap(),
            if merchant == DIGITAL_MERCHANT_NO {
                500
            } else {
                1500
            },
        );
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

    check_sales_matches(
        &sales,
        &sales_out,
        &receipts_out,
        &invoice_out,
        &uploads,
        &union,
    );
    check_finance_matches(finance, &union, &uploads, &invoice_out, &sales_out);
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

// §3.4: derive the expected key from source remarks, never from cleaned output.
fn expected_invoice_doc(remark: &str) -> String {
    use regex::Regex;
    use std::sync::LazyLock;

    // Canonicalize confirmed synonyms before independently scanning their values.
    let text = remark
        .replace("购机日期", "销售日期")
        .replace("购机时间", "销售日期")
        .replace("单据号收款号", "单据号")
        .replace("单据号收款", "单据号")
        .replace("单据收款号", "单据号")
        .replace("收款单号", "单据号")
        .replace("2026-0101", "2026-01-01")
        .replace("202601-04", "2026-01-04")
        .replace("2026-26-29", "2026-06-29");
    static CODE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"单据号[：:、\s]*((?:收款)?[A-Za-z0-9]+)").unwrap());
    let mut dates = HashSet::new();
    // Scan each canonical label independently of production regex extraction.
    for candidate in text.split("销售日期").skip(1) {
        let candidate = candidate
            .trim_start_matches(|c: char| c.is_whitespace() || matches!(c, '：' | ':' | '、'));
        let mut parts = candidate.splitn(3, ['-', '.', '/', '年', '月']);
        let year = parts.next().unwrap_or_default();
        let month = parts.next().unwrap_or_default();
        let day: String = parts
            .next()
            .unwrap_or_default()
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        if !matches!(year.len(), 2 | 4)
            || !(1..=2).contains(&month.len())
            || !(1..=2).contains(&day.len())
            || !year
                .bytes()
                .chain(month.bytes())
                .all(|b| b.is_ascii_digit())
        {
            return String::new();
        }
        let year: i32 = year.parse().unwrap();
        let Some(date) = NaiveDate::from_ymd_opt(
            if year < 100 { year + 2000 } else { year },
            month.parse().unwrap(),
            day.parse().unwrap(),
        ) else {
            return String::new();
        };
        dates.insert(date);
    }
    let codes: HashSet<_> = CODE
        .captures_iter(&text)
        .map(|c| {
            let s = &c[1];
            s.strip_prefix("收款").unwrap_or(s).to_string()
        })
        .filter(|c| c.bytes().any(|b| b.is_ascii_digit()))
        .collect();
    if dates.len() != 1 || codes.len() != 1 {
        return String::new();
    }
    let mut code = codes.into_iter().next().unwrap();
    if code == "ZFP300008" {
        code = "ZFP3000008".into();
    } else if code.len() == 11 {
        let Some(zero) = code.find('0') else {
            return String::new();
        };
        code.remove(zero);
    }
    if code.len() != 10 {
        return String::new();
    }
    format!(
        "{}{code}",
        dates.into_iter().next().unwrap().format("%y%m%d")
    )
}

fn invoice_output_record(table: &Table, row: &[Data]) -> Vec<String> {
    table.project(
        row,
        &[
            "开票类型",
            "数电发票号码",
            "购方名称",
            "主要商品名称",
            "备注信息",
            "开票状态",
            "匹配单据号",
            "大类",
            "品牌",
        ],
    )
}

fn check_invoice_fields(raw: &Table, out: &Table) -> Vec<Vec<String>> {
    assert_eq!(
        out.header,
        [
            "开票时间",
            "开票类型",
            "数电发票号码",
            "购方名称",
            "主要商品名称",
            "备注信息",
            "开票状态",
            "匹配单据号",
            "大类",
            "品牌",
        ],
        "发票输出表头必须与规定的10个字段名称和顺序一致"
    );
    // §3.3: the verified catalog is the expected-data source, not the product parser.
    let catalog: HashMap<_, _> = include_str!("../../src/utils/invoice_products.tsv")
        .lines()
        .skip(1)
        .map(|line| {
            let fields: Vec<_> = line.split('\t').collect();
            (fields[0], (fields[1], fields[2]))
        })
        .collect();
    let expected: Vec<_> = raw
        .rows
        .iter()
        .map(|row| {
            let mut record = invoice_record(raw, row);
            let (category, brand) = catalog.get(record[3].as_str()).unwrap_or_else(|| {
                panic!("发票商品缺少独立验收预期，请核实后补充清单：{}", record[3])
            });
            record.extend([
                expected_invoice_doc(&record[4]),
                category.to_string(),
                brand.to_string(),
            ]);
            record
        })
        .collect();
    multiset(
        out.rows
            .iter()
            .map(|r| invoice_output_record(out, r))
            .collect(),
        expected.clone(),
        "发票源字段、匹配单据号、大类或品牌错误",
    );
    expected
}

fn check_invoices(raw: &Table, out: &Table, path: &Path) {
    let expected = check_invoice_fields(raw, out);
    let abnormal = |t: &Table, r: &[Data]| {
        t.text(r, &["开票类型"]) == "红票"
            || ["已红冲", "开票失败"].contains(&t.text(r, &["开票状态"]).as_str())
    };
    let mut counts = HashMap::new();
    for (row, record) in raw.rows.iter().zip(&expected) {
        if !abnormal(raw, row) {
            let key = &record[6];
            if !key.is_empty() {
                *counts.entry(key).or_insert(0) += 1;
            }
        }
    }
    let mut groups: [Vec<Vec<String>>; 3] = std::array::from_fn(|_| Vec::new());
    for (row, record) in raw.rows.iter().zip(&expected) {
        let key = &record[6];
        let group = if abnormal(raw, row) {
            2
        } else if counts.get(key).copied().unwrap_or(0) > 1 {
            1
        } else {
            0
        };
        groups[group].push(record.clone());
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
            .map(|r| invoice_output_record(out, r))
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
    // Independent oracle: read verified TSV evidence directly, without calling ProductIdentity.
    let mut sales_catalog = HashMap::new();
    for line in include_str!("../../src/utils/sales_products.tsv")
        .lines()
        .skip(1)
    {
        let fields: Vec<_> = line.split('\t').collect();
        sales_catalog.insert(
            fields[0],
            (fields[2], fields[1], fields[0].starts_with("OXO")),
        );
    }
    let mut invoice_catalog = HashMap::new();
    for line in include_str!("../../src/utils/invoice_products.tsv")
        .lines()
        .skip(1)
    {
        let fields: Vec<_> = line.split('\t').collect();
        invoice_catalog.insert(fields[0], (fields[1], fields.get(4) == Some(&"非商品费用")));
    }
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
        let name = raw.text(row, &["商品名称"]);
        let source_brand = raw.text(row, &["品牌"]);
        let source_category = raw.text(row, &["财务大类"]);
        let brand = match source_brand.as_str() {
            "COLMO厨热JX" | "美的厨热JX" | "东芝JX" | "华凌" | "小天鹅" | "COLMO" => {
                "美的"
            }
            "卡萨帝" | "统帅" => "海尔",
            "晶弘" => "格力",
            value => value,
        };
        let category = match source_category.as_str() {
            "国产彩电" | "进口彩电" => "彩电",
            "新业务类" => "数码",
            value => value,
        };
        let (brand, category) =
            if let Some((b, c, explicit_blank)) = sales_catalog.get(name.as_str()) {
                (
                    if *explicit_blank {
                        ""
                    } else if !b.is_empty() {
                        *b
                    } else {
                        brand
                    },
                    if *explicit_blank || !c.is_empty() {
                        *c
                    } else {
                        category
                    },
                )
            } else if let Some((c, is_fee)) = invoice_catalog.get(name.as_str()) {
                if *is_fee {
                    ("", "")
                } else {
                    (brand, if !c.is_empty() { *c } else { category })
                }
            } else {
                (brand, category)
            };
        partition[usize::from(pink)].push(vec![
            doc,
            name,
            brand.to_string(),
            category.to_string(),
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
                out.text(row, &["品牌"]),
                out.text(row, &["财务大类"]),
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
    let mut categories = HashMap::new();
    for (evidence, explicit_blank) in [
        (include_str!("../../src/utils/invoice_products.tsv"), false),
        (include_str!("../../src/utils/sales_products.tsv"), true),
    ] {
        for line in evidence.lines().skip(1) {
            let mut fields = line.split('\t');
            let name = fields.next().unwrap();
            let category = fields.next().unwrap();
            let evidence = fields.nth(2).unwrap_or("");
            if explicit_blank || evidence == "非商品费用" || !category.is_empty() {
                categories.insert(name, category);
            }
        }
    }
    for row in &raw_sales.rows {
        let name = raw_sales.text(row, &["商品名称"]);
        let original = raw_sales.text(row, &["财务大类"]);
        let category = categories.get(name.as_str()).copied().unwrap_or(&original);
        let digital = ["数码", "新业务类"].contains(&category);
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
    // Only occurrence amounts use coupon columns; occurrence counts retain finance-category routing.
    metrics[0][0].0 = raw_sales
        .rows
        .iter()
        .map(|row| money(raw_sales.cell(row, &["2026家电国补（计入收入）"])))
        .sum();
    metrics[0][1].0 = raw_sales
        .rows
        .iter()
        .map(|row| money(raw_sales.cell(row, &["2026数码国补（计入收入）"])))
        .sum();
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

fn unique(values: Option<&HashSet<String>>) -> String {
    values
        .filter(|values| values.len() == 1)
        .and_then(|values| values.iter().next())
        .cloned()
        .unwrap_or_default()
}

fn index_values(table: &Table, key: &str, value: &str) -> HashMap<String, HashSet<String>> {
    let mut index: HashMap<String, HashSet<String>> = HashMap::new();
    for row in &table.rows {
        let key = table.text(row, &[key]);
        let value = table.text(row, &[value]);
        if !key.is_empty() && !value.is_empty() {
            index.entry(key).or_default().insert(value);
        }
    }
    index
}

fn check_upload_matches(raw: &[Table], out: &Table, refund: &Table, pink: &[bool], cap: i64) {
    // §5.7: order first, reference second; ambiguous amounts stop fallback; pink refunds excluded.
    let mut orders: HashMap<String, HashSet<String>> = HashMap::new();
    let mut references: HashMap<String, HashSet<String>> = HashMap::new();
    for (row, pink) in refund.rows.iter().zip(pink) {
        if *pink {
            continue;
        }
        let amount = money_text(refund.cell(row, &["补贴金额"]));
        for (index, field) in [(&mut orders, "商户订单号"), (&mut references, "交易参考号")]
        {
            let key = refund.text(row, &[field]);
            if !key.is_empty() {
                index.entry(key).or_default().insert(amount.clone());
            }
        }
    }
    let expected = raw
        .iter()
        .flat_map(|table| {
            table.rows.iter().map(|row| {
                let matched = [(&orders, "订单号"), (&references, "检索参考号")]
                    .iter()
                    .find_map(|(index, field)| index.get(&table.text(row, &[*field])));
                let actual_amount = unique(matched);
                let status = if !actual_amount.is_empty() {
                    "已回款".to_string()
                } else {
                    match table.text(row, &["状态"]).as_str() {
                        "审核通过" => "审核通过未回款".to_string(),
                        "同步(已上送)" | "暂存" | "待同步" | "核销成功" => {
                            "待审核".to_string()
                        }
                        "核销失败" => "审核失败".to_string(),
                        value => value.to_string(),
                    }
                };
                let transaction = table.cell(row, &["交易金额"]);
                let amount = if !actual_amount.is_empty() {
                    actual_amount
                } else if status == "审核终止" || transaction.to_string().trim().is_empty() {
                    String::new()
                } else {
                    (Decimal::from_str(&transaction.to_string()).unwrap() * Decimal::new(15, 2))
                        .round_dp(2)
                        .min(Decimal::from(cap))
                        .normalize()
                        .to_string()
                };
                vec![table.text(row, &["实时清分UUID"]), status, amount]
            })
        })
        .collect();
    multiset(
        out.rows
            .iter()
            .map(|row| {
                vec![
                    out.text(row, &["实时清分UUID"]),
                    out.text(row, &["状态"]),
                    money_text(out.cell(row, &["补贴金额"])),
                ]
            })
            .collect(),
        expected,
        "上传补贴金额、状态与审核终止豁免",
    );
}

fn check_sales_matches(
    raw: &Table,
    out: &Table,
    receipts: &Table,
    invoices: &Table,
    uploads: &[Table],
    union: &Table,
) {
    // §10.6/10.10/10.12: independently extract authority-validated references and apply all stages.
    let authority: HashSet<_> = union
        .rows
        .iter()
        .map(|r| union.text(r, &["检索号"]))
        .collect();
    let references = regex::Regex::new(r"[0-9]{11}N").unwrap();
    let digits = regex::Regex::new(r"[0-9]+").unwrap();
    let invoice_index = index_values(invoices, "匹配单据号", "数电发票号码");
    let receipts_index = index_values(receipts, "匹配单据号", "备注");
    let upload_indices: Vec<_> = uploads
        .iter()
        .map(|t| {
            (
                index_values(t, "检索参考号", "状态"),
                index_values(t, "发票号码", "状态"),
            )
        })
        .collect();
    // Product classification itself is independently verified by check_sales above.
    let categories: HashMap<_, _> = out
        .rows
        .iter()
        .map(|r| {
            (
                (out.text(r, &["匹配单据号"]), out.text(r, &["商品名称"])),
                out.text(r, &["财务大类"]),
            )
        })
        .collect();
    let expected = raw
        .rows
        .iter()
        .map(|row| {
            let summary = raw.text(row, &["明细摘要"]);
            let mut candidates: HashSet<String> = references
                .find_iter(&summary)
                .filter(|m| {
                    let before = summary[..m.start()].chars().next_back();
                    let after = summary[m.end()..].chars().next();
                    !before.is_some_and(|c| c.is_ascii_alphanumeric())
                        && !after.is_some_and(|c| c.is_ascii_alphanumeric())
                        && authority.contains(m.as_str())
                })
                .map(|m| m.as_str().to_string())
                .collect();
            if candidates.is_empty() {
                candidates = digits
                    .find_iter(&summary)
                    .filter(|m| m.len() == 11)
                    .map(|m| format!("{}N", m.as_str()))
                    .filter(|v| authority.contains(v))
                    .collect();
            }
            let reference = unique(Some(&candidates));
            let doc = raw.text(row, &["单据号"]);
            let key = match_no(raw.cell(row, &["单据日期"]), &doc);
            let name = raw.text(row, &["商品名称"]);
            let invoice = unique(invoice_index.get(&key));
            let mut remark = unique(receipts_index.get(&key));
            if remark.is_empty() {
                let group = usize::from(categories[&(key.clone(), name.clone())] == "数码");
                let (by_reference, by_invoice) = &upload_indices[group];
                remark = unique(by_reference.get(&reference));
                if remark.is_empty() {
                    remark = unique(by_invoice.get(&invoice));
                }
            }
            if remark.is_empty() {
                remark = "未上传".into();
            }
            vec![doc, name, key, reference, invoice, remark]
        })
        .collect();
    multiset(
        out.rows
            .iter()
            .map(|r| {
                out.project(
                    r,
                    &[
                        "单据号",
                        "商品名称",
                        "匹配单据号",
                        "参考号",
                        "数电发票号码",
                        "备注",
                    ],
                )
            })
            .collect(),
        expected,
        "销售参考号、发票关联与备注级联",
    );
}

fn index_records(
    table: &Table,
    key: &str,
    fields: &[&str],
) -> HashMap<String, HashSet<Vec<String>>> {
    let mut index: HashMap<String, HashSet<Vec<String>>> = HashMap::new();
    for row in &table.rows {
        let key = table.text(row, &[key]);
        if !key.is_empty() {
            index
                .entry(key)
                .or_default()
                .insert(table.project(row, fields));
        }
    }
    index
}

fn unique_record(values: Option<&HashSet<Vec<String>>>) -> Option<&Vec<String>> {
    values
        .filter(|values| values.len() == 1)
        .and_then(|values| values.iter().next())
}

fn check_finance_matches(
    path: &Path,
    union: &Table,
    uploads: &[Table],
    invoices: &Table,
    sales: &Table,
) {
    // Reporting §5.1.1: unique sales products, uploaded invoice first, preferred blue fallback,
    // return status override, and red-flush flag; conflicting candidates never select an arbitrary row.
    let mut uploaded: HashMap<String, HashSet<Vec<String>>> = HashMap::new();
    for table in uploads {
        for (key, records) in index_records(table, "检索参考号", &["状态", "发票号码"]) {
            uploaded.entry(key).or_default().extend(records);
        }
    }
    let invoice_index = index_records(
        invoices,
        "数电发票号码",
        &["开票类型", "开票状态", "大类", "品牌", "主要商品名称"],
    );
    let product_index = index_records(sales, "参考号", &["财务大类", "品牌", "商品名称"]);
    let documents = index_values(sales, "参考号", "匹配单据号");
    let mut blue: HashMap<String, HashSet<String>> = HashMap::new();
    let mut completed: HashMap<String, HashSet<String>> = HashMap::new();
    for row in &invoices.rows {
        let key = invoices.text(row, &["匹配单据号"]);
        let number = invoices.text(row, &["数电发票号码"]);
        if !key.is_empty() && !number.is_empty() && invoices.text(row, &["开票类型"]) == "蓝票"
        {
            blue.entry(key.clone()).or_default().insert(number.clone());
            if invoices.text(row, &["开票状态"]) == "开票完成" {
                completed.entry(key).or_default().insert(number);
            }
        }
    }
    let final_table = Table::read(path, 0, "序号", false);
    let mut expected = Vec::new();
    for (i, row) in union.rows.iter().enumerate() {
        let reference = union.text(row, &["检索号"]);
        let upload = unique_record(uploaded.get(&reference));
        let mut number = upload.map(|r| r[1].clone()).unwrap_or_default();
        if number.is_empty() {
            let doc = unique(documents.get(&reference));
            number = unique(completed.get(&doc).or_else(|| blue.get(&doc)));
        }
        let invoice = unique_record(invoice_index.get(&number));
        let product = unique_record(product_index.get(&reference))
            .filter(|r| r.iter().all(|field| !field.is_empty()))
            .cloned()
            .unwrap_or_else(|| {
                invoice
                    .map(|r| r[2..].to_vec())
                    .unwrap_or_else(|| vec![String::new(); 3])
            });
        let status = if union.text(row, &["备注"]) == "已退货" {
            "已退货".to_string()
        } else {
            upload
                .map(|r| r[0].clone())
                .unwrap_or_else(|| "未提交".into())
        };
        let red = if invoice.is_some_and(|r| r[0] == "红票" || r[1] == "已红冲") {
            "是"
        } else {
            ""
        };
        let mut record = vec![(i + 1).to_string()];
        record.extend(product);
        record.extend([status, number, red.into()]);
        expected.push(record);
    }
    ordered(
        final_table
            .rows
            .iter()
            .map(|r| {
                final_table.project(
                    r,
                    &[
                        "序号",
                        "大类",
                        "品牌",
                        "产品型号",
                        "状态",
                        "发票号",
                        "发票是否红冲",
                    ],
                )
            })
            .collect(),
        expected,
        "财务最终匹配字段与流水顺序",
    );
}

#[cfg(test)]
mod oracle_tests {
    use super::*;

    fn table(header: &[&str], rows: &[&[&str]]) -> Table {
        Table {
            header: header.iter().map(|s| s.to_string()).collect(),
            rows: rows
                .iter()
                .map(|r| r.iter().map(|s| Data::String(s.to_string())).collect())
                .collect(),
        }
    }

    #[test]
    fn invoice_oracle_rejects_missing_wrong_and_guessed_fields() {
        let header = [
            "开票时间",
            "开票类型",
            "数电发票号码",
            "购方名称",
            "主要商品名称",
            "备注信息",
            "开票状态",
            "匹配单据号",
            "大类",
            "品牌",
        ];
        let raw = table(
            &header[1..7],
            &[
                &[
                    "蓝票",
                    "INV1",
                    "买方",
                    "*商品*OPPO手机",
                    "购机时间：2026年3月19日 收款单号:ZFMT000014",
                    "开票完成",
                ],
                &[
                    "蓝票",
                    "INV2",
                    "买方",
                    "服务费",
                    "销售日期：3月19日 单号:ZFMT000014",
                    "开票完成",
                ],
                &["蓝票", "INV3", "买方", "手机", "", "开票完成"],
            ],
        );
        let mut out = table(
            &header,
            &[
                &[
                    "2026-03-19 14:00:00",
                    "蓝票",
                    "INV1",
                    "买方",
                    "OPPO手机",
                    "购机时间：2026年3月19日 收款单号:ZFMT000014",
                    "开票完成",
                    "260319ZFMT000014",
                    "数码",
                    "OPPO",
                ],
                &[
                    "2026-03-19 14:00:00",
                    "蓝票",
                    "INV2",
                    "买方",
                    "服务费",
                    "销售日期：3月19日 单号:ZFMT000014",
                    "开票完成",
                    "",
                    "",
                    "",
                ],
                &[
                    "2026-03-19 14:00:00",
                    "蓝票",
                    "INV3",
                    "买方",
                    "手机",
                    "",
                    "开票完成",
                    "",
                    "数码",
                    "",
                ],
            ],
        );
        check_invoice_fields(&raw, &out);
        // §3.3–3.4: reject missing/wrong extracted fields and invented values for intentional blanks.
        for (row, col, bad) in [
            (0, 7, ""),
            (0, 7, "260319ZFMT000015"),
            (0, 8, ""),
            (0, 8, "电脑"),
            (0, 9, ""),
            (0, 9, "vivo"),
            (1, 7, "260319ZFMT000014"),
            (1, 8, "数码"),
            (1, 9, "OPPO"),
            (2, 9, "OPPO"),
        ] {
            let original = out.rows[row][col].clone();
            out.rows[row][col] = Data::String(bad.into());
            assert!(
                std::panic::catch_unwind(|| check_invoice_fields(&raw, &out)).is_err(),
                "row {row}, col {col}, bad {bad}"
            );
            out.rows[row][col] = original;
        }
        let mut bad_header = out.clone();
        bad_header.header.swap(0, 1);
        assert!(std::panic::catch_unwind(|| check_invoice_fields(&raw, &bad_header)).is_err());
        out.rows.push(out.rows[0].clone());
        assert!(std::panic::catch_unwind(|| check_invoice_fields(&raw, &out)).is_err());
    }

    #[test]
    fn invoice_oracle_obeys_date_length_and_ambiguity_rules() {
        for (remark, expected) in [
            ("购机日期：26.02.16 单据号：ZEXQ000062", "260216ZEXQ000062"),
            (
                "销售日期：2026-0101 单据号：收款ZG2J000424",
                "260101ZG2J000424",
            ),
            (
                "销售日期：202601-04 单据号收款号：ZHLT0000524",
                "260104ZHLT000524",
            ),
            (
                "销售日期：2026-26-29 单据号收款：ZFP300008",
                "260629ZFP3000008",
            ),
            (
                "购机时间：2026-5月31日 收款单号：ZH700000103",
                "260531ZH70000103",
            ),
            (
                "开具2026年2月27日购物发票 ,销售日期2026年2月27日 收款单号：ZFMT000031收款方式：微信支付",
                "260227ZFMT000031",
            ),
            (
                "销售日期：2026-03-19 购机时间：2026-03-19 单据号：ZFMT000014 收款单号：ZFMT000014",
                "260319ZFMT000014",
            ),
            (
                "销售日期：2026-03-19 单据号：ZFMT000014 收款单号：收款ZFMT000014",
                "260319ZFMT000014",
            ),
            (
                "销售日期：2026-03-18 购机时间：2026-03-19 收款单号：ZFMT000014",
                "",
            ),
            (
                "购机时间：2026-03-19 单据号：ZFMT000014 收款单号：ZFMT000015",
                "",
            ),
            ("开具2026年3月19日购物发票 收款单号：ZFMT000014", ""),
            ("购机时间：2026-02-30 收款单号：ZFMT000014", ""),
            ("购机时间：2026-03-190 收款单号：ZFMT000014", ""),
            ("销售日期：2026-12-310 单据号：ZFMT000014", ""),
            ("销售日期：202-03-19 单据号：ZFMT000014", ""),
            ("销售日期：20260-03-19 单据号：ZFMT000014", ""),
            ("销售日期：2026-003-19 单据号：ZFMT000014", ""),
            (
                "销售日期：2026-03-19 购机时间：2026-02-30 收款单号：ZFMT000014",
                "",
            ),
            (
                "购机时间：2026-02-30 销售日期：2026-03-19 收款单号：ZFMT000014",
                "",
            ),
            (
                "销售日期：2026-03-19 购机日期：3月19日 收款单号：ZFMT000014",
                "",
            ),
            (
                "销售日期：2026-03-19 购机时间：未知 收款单号：ZFMT000014",
                "",
            ),
            ("购机时间：2026-03-19 收款单号：ZFMT00014", ""),
            ("购机时间：2026-03-19 收款单号：ZFMT1234567", ""),
            ("购机时间：2026-03-19 收款单号：ABCDEFGHIJ", ""),
        ] {
            assert_eq!(expected_invoice_doc(remark), expected, "{remark}");
        }
    }

    #[test]
    fn upload_oracle_rejects_wrong_actual_subsidy() {
        let raw = vec![table(
            &["实时清分UUID", "订单号", "检索参考号", "状态", "交易金额"],
            &[&["uuid", "order", "ref", "审核通过", "1000"]],
        )];
        let refund = table(
            &["商户订单号", "交易参考号", "补贴金额"],
            &[&["order", "ref", "20"]],
        );
        let mut out = table(
            &["实时清分UUID", "状态", "补贴金额"],
            &[&["uuid", "已回款", "20"]],
        );
        check_upload_matches(&raw, &out, &refund, &[false], 500);
        out.rows[0][2] = Data::String("150".into());
        assert!(
            std::panic::catch_unwind(|| check_upload_matches(&raw, &out, &refund, &[false], 500))
                .is_err()
        );
    }

    #[test]
    fn sales_oracle_rejects_wrong_status_after_reference_hit() {
        let raw = table(
            &["单据号", "单据日期", "商品名称", "明细摘要"],
            &[&["A", "2026-10-13", "产品", "12345678901"]],
        );
        let mut out = table(
            &[
                "单据号",
                "商品名称",
                "匹配单据号",
                "财务大类",
                "参考号",
                "数电发票号码",
                "备注",
            ],
            &[&[
                "A",
                "产品",
                "261013A",
                "数码",
                "12345678901N",
                "INV",
                "已回款",
            ]],
        );
        let receipts = table(&["匹配单据号", "备注"], &[]);
        let invoices = table(&["匹配单据号", "数电发票号码"], &[&["261013A", "INV"]]);
        let uploads = vec![
            table(&["检索参考号", "发票号码", "状态"], &[]),
            table(
                &["检索参考号", "发票号码", "状态"],
                &[&["12345678901N", "INV", "已回款"]],
            ),
        ];
        let union = table(&["检索号"], &[&["12345678901N"]]);
        check_sales_matches(&raw, &out, &receipts, &invoices, &uploads, &union);
        out.rows[0][6] = Data::String("未上传".into());
        assert!(
            std::panic::catch_unwind(|| check_sales_matches(
                &raw, &out, &receipts, &invoices, &uploads, &union
            ))
            .is_err()
        );
    }

    #[test]
    fn finance_oracle_rejects_wrong_blue_invoice_fallback() {
        let union = table(&["检索号", "备注"], &[&["REF", ""]]);
        let uploads = vec![table(&["检索参考号", "状态", "发票号码"], &[])];
        let invoices = table(
            &[
                "数电发票号码",
                "匹配单据号",
                "开票类型",
                "开票状态",
                "大类",
                "品牌",
                "主要商品名称",
            ],
            &[&["INV", "DOC", "蓝票", "开票完成", "电脑", "品牌", "发票产品"]],
        );
        let sales = table(
            &["参考号", "匹配单据号", "财务大类", "品牌", "商品名称"],
            &[&["REF", "DOC", "电脑", "品牌", "销售产品"]],
        );
        let path = std::env::temp_dir().join(format!(
            "subsidy-finance-oracle-{}-{}.xlsx",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let write = |invoice: &str| {
            let mut book = rust_xlsxwriter::Workbook::new();
            let sheet = book.add_worksheet();
            for (col, header) in [
                "序号",
                "大类",
                "品牌",
                "产品型号",
                "状态",
                "发票号",
                "发票是否红冲",
            ]
            .iter()
            .enumerate()
            {
                sheet.write_string(0, col as u16, *header).unwrap();
            }
            for (col, value) in ["1", "电脑", "品牌", "销售产品", "未提交", invoice, ""]
                .iter()
                .enumerate()
            {
                sheet.write_string(1, col as u16, *value).unwrap();
            }
            book.save(&path).unwrap();
        };
        write("INV");
        check_finance_matches(&path, &union, &uploads, &invoices, &sales);
        write("WRONG");
        assert!(
            std::panic::catch_unwind(|| check_finance_matches(
                &path, &union, &uploads, &invoices, &sales
            ))
            .is_err()
        );
        std::fs::remove_file(path).unwrap();
    }
}
