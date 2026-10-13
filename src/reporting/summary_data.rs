use super::data::SheetData;
use super::reader::{UploadColumns, cell_to_decimal, cell_to_string};
use super::unique_index::UniqueIndex;
use calamine::Data;
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use std::collections::{BTreeMap, HashMap, HashSet};

type UploadTotals = HashMap<String, (Decimal, i64)>;

const STD_CATEGORIES: [(&str, &[&str]); 7] = [
    (
        "厨卫",
        &["AO史密斯", "万家乐", "方太", "欧意", "海尔", "美的", "老板"],
    ),
    (
        "洗衣机",
        &["博世", "小鸭", "海尔", "美的", "美菱", "西门子"],
    ),
    ("冰箱", &["博世", "海信", "海尔", "美的", "美菱", "西门子"]),
    ("彩电", &["TCL", "创维", "华为（终端）", "海信", "海尔"]),
    (
        "空调",
        &["TCL", "奥克斯", "格力", "海信", "海尔", "科龙", "美的"],
    ),
    ("小电", &["沁园"]),
    (
        "数码",
        &[
            "IQOO",
            "OPPO",
            "VIVO",
            "一加",
            "作业帮",
            "华为",
            "学而思",
            "小天才",
            "小米",
            "步步高",
            "真我（REALME）",
            "苹果",
            "荣耀",
        ],
    ),
];

pub fn div_pct(num: Decimal, den: Decimal) -> f64 {
    if den.is_zero() {
        0.0
    } else {
        (num / den).to_f64().unwrap_or(0.0)
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct MetricRow {
    pub app_amt: Decimal,
    pub app_cnt: i64,
    pub dig_amt: Decimal,
    pub dig_cnt: i64,
}

impl MetricRow {
    pub fn new(app_amt: Decimal, app_cnt: i64, dig_amt: Decimal, dig_cnt: i64) -> Self {
        Self {
            app_amt,
            app_cnt,
            dig_amt,
            dig_cnt,
        }
    }

    fn subtract(self, other: Self) -> Self {
        Self::new(
            self.app_amt - other.app_amt,
            self.app_cnt - other.app_cnt,
            self.dig_amt - other.dig_amt,
            self.dig_cnt - other.dig_cnt,
        )
    }

    pub fn tot_amt(&self) -> Decimal {
        self.app_amt + self.dig_amt
    }

    pub fn tot_cnt(&self) -> i64 {
        self.app_cnt + self.dig_cnt
    }
}

#[derive(Debug, Clone)]
pub struct SummaryMetrics {
    pub occur: MetricRow,
    pub paid: MetricRow,
    pub unpaid: MetricRow,
    pub pass: MetricRow,
    pub wait: MetricRow,
    pub fail: MetricRow,
    pub unup: MetricRow,
}

impl SummaryMetrics {
    /// 数值顺序：发生、已回款、综合回款、未回款、通过、待审、失败、未上传。
    pub(crate) fn amount_ratios(&self) -> [(Decimal, f64, Decimal, f64, Decimal, f64); 8] {
        let combined = MetricRow::new(
            self.paid.app_amt + self.pass.app_amt,
            0,
            self.paid.dig_amt + self.pass.dig_amt,
            0,
        );
        let total = self.occur.tot_amt();
        let ratio = |row: &MetricRow, denominator: &MetricRow| {
            (
                row.app_amt,
                div_pct(row.app_amt, denominator.app_amt),
                row.dig_amt,
                div_pct(row.dig_amt, denominator.dig_amt),
                row.tot_amt(),
                div_pct(row.tot_amt(), denominator.tot_amt()),
            )
        };
        [
            // 合计占比也遵循零分母为零的规则。
            (
                self.occur.app_amt,
                div_pct(self.occur.app_amt, total),
                self.occur.dig_amt,
                div_pct(self.occur.dig_amt, total),
                total,
                div_pct(total, total),
            ),
            ratio(&self.paid, &self.occur),
            ratio(&combined, &self.occur),
            ratio(&self.unpaid, &self.occur),
            ratio(&self.pass, &self.unpaid),
            ratio(&self.wait, &self.unpaid),
            ratio(&self.fail, &self.unpaid),
            ratio(&self.unup, &self.unpaid),
        ]
    }

    /// 汇总 Sheet 的发生额按原始补贴券收入列计算；发生笔数及其他表仍用清洗后的财务大类。
    pub(crate) fn calculate_with_source(
        sales: &SheetData,
        app_up: &SheetData,
        dig_up: &SheetData,
        source: &std::path::Path,
    ) -> Result<Self, String> {
        let mut metrics = Self::calculate(sales, app_up, dig_up)?;
        let sheet = crate::jobs::coupons::load_source(source).map_err(|e| e.to_string())?;
        let last = sheet.last_value_row().unwrap_or(2);
        let mut totals = [Decimal::ZERO; 2];
        for row in 3..last {
            for (total, col) in totals.iter_mut().zip([26, 27]) {
                let cell = sheet.cell(row, col);
                let amount = crate::jobs::cell_amount(&cell)
                    .map_err(|e| format!("销售用券源表第{row}行第{col}列：{e}"))?
                    .unwrap_or(Decimal::ZERO);
                *total += crate::utils::numbers::to_cents(amount)
                    .ok_or_else(|| format!("销售用券源表第{row}行第{col}列金额尾差超出容差"))?;
            }
        }
        metrics.occur.app_amt = totals[0];
        metrics.occur.dig_amt = totals[1];
        metrics.unpaid = metrics.occur.subtract(metrics.paid);
        metrics.unup = metrics
            .unpaid
            .subtract(metrics.pass)
            .subtract(metrics.wait)
            .subtract(metrics.fail);
        Ok(metrics)
    }

    pub(crate) fn calculate(
        sales: &SheetData,
        app_up: &SheetData,
        dig_up: &SheetData,
    ) -> Result<Self, String> {
        let sales_h = &sales.header;
        let cat_idx = sales_h.find(&["财务大类"]).ok_or("销售表缺少财务大类")?;
        let sub_idx = sales_h.find(&["补贴额"]).ok_or("销售表缺少补贴额")?;
        let qty_idx = sales_h.find(&["数量"]).ok_or("销售表缺少数量")?;

        let mut app_gen_amt = Decimal::ZERO;
        let mut app_gen_cnt = Decimal::ZERO;
        let mut dig_gen_amt = Decimal::ZERO;
        let mut dig_gen_cnt = Decimal::ZERO;

        for row in &sales[1..] {
            let cat = cell_to_string(&row[cat_idx]);
            let amt = cell_to_decimal(&row[sub_idx]).unwrap_or(Decimal::ZERO);
            let qty = cell_to_decimal(&row[qty_idx]).unwrap_or(Decimal::ZERO);

            if cat == "数码" {
                dig_gen_amt += amt;
                dig_gen_cnt += qty;
            } else {
                app_gen_amt += amt;
                app_gen_cnt += qty;
            }
        }

        let count_uploaded = |rows: &SheetData, label: &str| -> Result<UploadTotals, String> {
            let cols = UploadColumns::from_header(&rows.header, label)?;
            let mut totals = UploadTotals::new();
            for row in &rows[1..] {
                let status = cell_to_string(&row[cols.status]);
                let amount = cell_to_decimal(&row[cols.subsidy]).unwrap_or(Decimal::ZERO);
                let entry = totals.entry(status).or_default();
                entry.0 += amount;
                entry.1 += 1;
            }
            Ok(totals)
        };

        let app = count_uploaded(app_up, "已上传家电电脑.xlsx")?;
        let dig = count_uploaded(dig_up, "已上传数码.xlsx")?;
        let by_status = |status: &str| {
            let (app_amt, app_cnt) = app.get(status).copied().unwrap_or_default();
            let (dig_amt, dig_cnt) = dig.get(status).copied().unwrap_or_default();
            MetricRow::new(app_amt, app_cnt, dig_amt, dig_cnt)
        };
        let occur = MetricRow::new(
            app_gen_amt,
            app_gen_cnt.to_i64().unwrap_or(0),
            dig_gen_amt,
            dig_gen_cnt.to_i64().unwrap_or(0),
        );
        let paid = by_status("已回款");
        let unpaid = occur.subtract(paid);
        let pass = by_status("审核通过未回款");
        let wait = by_status("待审核");
        let fail = by_status("审核失败");
        let unup = unpaid.subtract(pass).subtract(wait).subtract(fail);
        Ok(Self {
            occur,
            paid,
            unpaid,
            pass,
            wait,
            fail,
            unup,
        })
    }
}

pub(crate) const SUMMARY_STATUSES: [&str; 5] =
    ["已回款", "审核通过未回款", "待审核", "审核失败", "未上传"];

pub(crate) struct BrandSummary {
    pub brand: String,
    pub totals: [(Decimal, i64); 5],
}

pub(crate) struct CategorySummary {
    pub category: String,
    pub brands: Vec<BrandSummary>,
}

/// 品类品牌汇总：聚合、业务排序与五状态投影由同一 module 拥有。
/// Excel 格式与大类合并由渲染负责。
pub(crate) struct CategoryBrandSummary {
    groups: Vec<CategorySummary>,
}

impl CategoryBrandSummary {
    pub fn from_sales(sales: &SheetData) -> Result<Self, String> {
        let h = &sales.header;
        let cat_idx = h.require("财务大类", "销售用券情况统计.xlsx")?;
        let brand_idx = h.require("品牌", "销售用券情况统计.xlsx")?;
        let sub_idx = h.require("补贴额", "销售用券情况统计.xlsx")?;
        let qty_idx = h.require("数量", "销售用券情况统计.xlsx")?;
        let remark_idx = h.require("备注", "销售用券情况统计.xlsx")?;

        let mut categories: BTreeMap<String, BTreeMap<String, [(Decimal, i64); 5]>> =
            BTreeMap::new();
        for row in &sales[1..] {
            let remark = cell_to_string(&row[remark_idx]);
            if remark == "退货-原单" || remark == "退货-退单" {
                continue;
            }
            let totals = categories
                .entry(cell_to_string(&row[cat_idx]))
                .or_default()
                .entry(cell_to_string(&row[brand_idx]))
                .or_default();
            // 非展示状态仍保留实际品类品牌组合，但不计入五状态数值。
            if let Some(status) = SUMMARY_STATUSES.iter().position(|value| *value == remark) {
                totals[status].0 += cell_to_decimal(&row[sub_idx]).unwrap_or(Decimal::ZERO);
                totals[status].1 += cell_to_decimal(&row[qty_idx])
                    .and_then(|d| d.to_i64())
                    .unwrap_or(0);
            }
        }

        let mut groups: Vec<_> = categories
            .into_iter()
            .map(|(category, brands)| {
                let standard_brands = STD_CATEGORIES
                    .iter()
                    .find(|(name, _)| *name == category)
                    .map(|(_, brands)| *brands)
                    .unwrap_or_default();
                let mut brands: Vec<_> = brands
                    .into_iter()
                    .map(|(brand, totals)| BrandSummary { brand, totals })
                    .collect();
                // BTreeMap 已按文本升序提供新增品牌；稳定排序将标准品牌移至业务位置。
                brands.sort_by_key(|brand| {
                    standard_brands
                        .iter()
                        .position(|name| *name == brand.brand)
                        .unwrap_or(usize::MAX)
                });
                CategorySummary { category, brands }
            })
            .collect();
        // 标准品类使用业务顺序，新增品类保持 BTreeMap 的文本升序。
        groups.sort_by_key(|group| {
            STD_CATEGORIES
                .iter()
                .position(|(name, _)| *name == group.category)
                .unwrap_or(usize::MAX)
        });
        Ok(Self { groups })
    }

    pub fn groups(&self) -> &[CategorySummary] {
        &self.groups
    }
}

pub(crate) fn build_invoice_name_index(
    invoices: &SheetData,
) -> Result<UniqueIndex<String>, String> {
    let inv_h = &invoices.header;
    let inv_no_idx = inv_h
        .find(&["数电发票号码"])
        .ok_or("发票明细缺少数电发票号码")?;
    let inv_name_idx = inv_h
        .find(&["主要商品名称"])
        .ok_or("发票明细缺少主要商品名称")?;
    let mut index = UniqueIndex::default();
    for row in &invoices[1..] {
        let no = cell_to_string(&row[inv_no_idx]);
        let name = cell_to_string(&row[inv_name_idx]);
        if !no.is_empty() {
            index.insert(no, name);
        }
    }
    Ok(index)
}

pub(crate) struct AnomalyColors {
    pub app_pink: HashSet<u32>,
    pub dig_pink: HashSet<u32>,
    pub invoice_yellow: HashSet<u32>,
    pub invoice_pink: HashSet<u32>,
}

/// 审核失败明细行模型（包含已关联商品名称或数码 IMEI）
#[derive(Debug, PartialEq, Clone)]
pub(crate) struct FailedUploadRecord<'a> {
    pub row: &'a [Data],
    pub extra1: String,
    pub extra2: Option<String>,
}

/// 异常回款明细节模型（封装参考号多重度分组与排序）
#[derive(Debug, PartialEq, Clone)]
pub(crate) struct RefundAnomalySection<'a> {
    pub rows: Vec<&'a [Data]>,
    pub ref_col_idx: usize,
}

/// 异常发票明细节模型（封装按匹配单据号排序）
#[derive(Debug, PartialEq, Clone)]
pub(crate) struct InvoiceAnomalySection<'a> {
    pub rows: Vec<&'a [Data]>,
}

/// 汇总报表 Sheets 3–5 异常数据的深度领域模型
#[derive(Debug, PartialEq, Clone)]
pub(crate) struct AnomalyDatasets<'a> {
    pub app_failed: Vec<FailedUploadRecord<'a>>,
    pub dig_failed: Vec<FailedUploadRecord<'a>>,
    pub app_refund: RefundAnomalySection<'a>,
    pub dig_refund: RefundAnomalySection<'a>,
    pub invoices: InvoiceAnomalySection<'a>,
}

impl<'a> AnomalyDatasets<'a> {
    pub fn extract(
        sources: [&'a SheetData; 5],
        colors: &AnomalyColors,
        stores: [&str; 2],
        invoice_names: &UniqueIndex<String>,
    ) -> Result<Self, String> {
        let [app_up, dig_up, invoices, app_refund, dig_refund] = sources;
        let [app_store, dig_store] = stores;

        let app_failed = FailedUploadRecord::extract_appliance(app_up, invoice_names)?;
        let dig_failed = FailedUploadRecord::extract_digital(dig_up)?;
        let app_refund_sec = RefundAnomalySection::extract(
            app_refund,
            &colors.app_pink,
            app_store,
            "回款明细家电电脑.xlsx",
        )?;
        let dig_refund_sec = RefundAnomalySection::extract(
            dig_refund,
            &colors.dig_pink,
            dig_store,
            "回款明细数码.xlsx",
        )?;
        let invoices_sec = InvoiceAnomalySection::extract(invoices, colors)?;

        Ok(Self {
            app_failed,
            dig_failed,
            app_refund: app_refund_sec,
            dig_refund: dig_refund_sec,
            invoices: invoices_sec,
        })
    }
}

impl<'a> FailedUploadRecord<'a> {
    pub(crate) fn extract_appliance(
        app_up: &'a SheetData,
        invoice_names: &UniqueIndex<String>,
    ) -> Result<Vec<Self>, String> {
        let status_idx = app_up.header.require("状态", "已上传明细")?;
        let inv_idx = app_up.header.require("发票号码", "已上传家电电脑.xlsx")?;

        let mut records: Vec<_> = app_up[1..]
            .iter()
            .filter(|row| cell_to_string(&row[status_idx]) == "审核失败")
            .map(|row| {
                let inv_no = cell_to_string(&row[inv_idx]);
                let prod_name = invoice_names.get(&inv_no).cloned().unwrap_or_default();
                FailedUploadRecord {
                    row: row.as_slice(),
                    extra1: prod_name,
                    extra2: None,
                }
            })
            .collect();

        records.sort_by_key(|r| (r.extra1.is_empty(), r.extra1.clone()));
        Ok(records)
    }

    pub(crate) fn extract_digital(dig_up: &'a SheetData) -> Result<Vec<Self>, String> {
        let status_idx = dig_up.header.require("状态", "已上传明细")?;
        let imei1_idx = dig_up
            .header
            .find(&["IMEI1", "imei1"])
            .ok_or("已上传数码.xlsx: 缺少必要列 [IMEI1]")?;
        let imei2_idx = dig_up
            .header
            .find(&["IMEI2", "imei2"])
            .ok_or("已上传数码.xlsx: 缺少必要列 [IMEI2]")?;

        let records: Vec<_> = dig_up[1..]
            .iter()
            .filter(|row| cell_to_string(&row[status_idx]) == "审核失败")
            .map(|row| {
                let imei1 = cell_to_string(&row[imei1_idx]);
                let imei2 = cell_to_string(&row[imei2_idx]);
                FailedUploadRecord {
                    row: row.as_slice(),
                    extra1: imei1,
                    extra2: Some(imei2),
                }
            })
            .collect();

        Ok(records)
    }
}

impl<'a> RefundAnomalySection<'a> {
    pub(crate) fn extract(
        refund: &'a SheetData,
        pink: &HashSet<u32>,
        store: &str,
        file_label: &str,
    ) -> Result<Self, String> {
        if refund.is_empty() {
            return Ok(RefundAnomalySection {
                rows: Vec::new(),
                ref_col_idx: 0,
            });
        }
        let merchant_idx = refund.header.require("核销商编", "回款明细")?;
        let ref_idx = refund
            .header
            .find(&["交易参考号"])
            .ok_or_else(|| format!("{file_label} 缺少交易参考号"))?;

        let mut matched_rows: Vec<&'a [Data]> = refund
            .iter()
            .enumerate()
            .skip(1)
            .filter(|(index, row)| {
                cell_to_string(&row[merchant_idx]) == store && pink.contains(&(*index as u32 + 1))
            })
            .map(|(_, row)| row.as_slice())
            .collect();

        let reference_counts = matched_rows.iter().fold(HashMap::new(), |mut counts, row| {
            *counts.entry(cell_to_string(&row[ref_idx])).or_insert(0) += 1;
            counts
        });

        matched_rows.sort_by_key(|row| {
            let reference = cell_to_string(&row[ref_idx]);
            (
                reference.is_empty(),
                reference_counts[&reference] == 1,
                reference,
            )
        });

        Ok(RefundAnomalySection {
            rows: matched_rows,
            ref_col_idx: ref_idx,
        })
    }
}

impl<'a> InvoiceAnomalySection<'a> {
    pub(crate) fn extract(invoices: &'a SheetData, colors: &AnomalyColors) -> Result<Self, String> {
        let doc_no_idx = invoices
            .header
            .find(&["匹配单据号"])
            .ok_or("发票明细缺少匹配单据号")?;

        let mut collected: Vec<&'a [Data]> = invoices
            .iter()
            .enumerate()
            .skip(1)
            .filter(|(index, _)| {
                let row_num = *index as u32 + 1;
                colors.invoice_yellow.contains(&row_num) && !colors.invoice_pink.contains(&row_num)
            })
            .map(|(_, row)| row.as_slice())
            .collect();

        collected.sort_by_key(|r| {
            let s = cell_to_string(&r[doc_no_idx]);
            (s.is_empty(), s)
        });

        Ok(InvoiceAnomalySection { rows: collected })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sales(rows: &[&[&str]]) -> SheetData {
        SheetData::new(
            std::iter::once(&["财务大类", "品牌", "补贴额", "数量", "备注"][..])
                .chain(rows.iter().copied())
                .map(|row| {
                    row.iter()
                        .map(|value| Data::String((*value).into()))
                        .collect()
                })
                .collect(),
        )
    }

    #[test]
    fn category_brand_summary_orders_actual_combinations_and_projects_five_statuses() {
        let input = sales(&[
            &["Z", "B", "900", "1", "其他"],
            &["冰箱", "海尔", "10.25", "1", "已回款"],
            &["冰箱", "海尔", "-2.25", "-1", "已回款"],
            &["冰箱", "博世", "5", "2", "待审核"],
            &["冰箱", "Z", "4", "1", "未上传"],
            &["冰箱", "A", "3", "1", "审核失败"],
            &["冰箱", "", "6", "1", "审核通过未回款"],
            &["A", "", "7", "1", ""],
            &["", "", "8", "1", "未上传"],
            &["退货品类", "退货品牌", "20", "1", " 退货-原单 "],
            &["退货品类", "退货品牌", "-20", "-1", "退货-退单"],
        ]);
        let summary = CategoryBrandSummary::from_sales(&input).unwrap();
        let names: Vec<_> = summary
            .groups()
            .iter()
            .flat_map(|group| {
                group
                    .brands
                    .iter()
                    .map(move |brand| (group.category.as_str(), brand.brand.as_str()))
            })
            .collect();
        assert_eq!(
            names,
            [
                ("冰箱", "博世"),
                ("冰箱", "海尔"),
                ("冰箱", ""),
                ("冰箱", "A"),
                ("冰箱", "Z"),
                ("", ""),
                ("A", ""),
                ("Z", "B")
            ]
        );
        let zero = (Decimal::ZERO, 0);
        let brands = &summary.groups()[0].brands;
        assert_eq!(
            brands[0].totals,
            [zero, zero, (Decimal::from(5), 2), zero, zero]
        );
        assert_eq!(
            brands[1].totals,
            [(Decimal::from(8), 0), zero, zero, zero, zero]
        );
        assert_eq!(
            brands[2].totals,
            [zero, (Decimal::from(6), 1), zero, zero, zero]
        );
        assert_eq!(
            brands[3].totals,
            [zero, zero, zero, (Decimal::from(3), 1), zero]
        );
        assert_eq!(
            brands[4].totals,
            [zero, zero, zero, zero, (Decimal::from(4), 1)]
        );
        assert_eq!(
            summary.groups()[1].brands[0].totals[4],
            (Decimal::from(8), 1)
        );
        assert_eq!(summary.groups()[2].brands[0].totals, [zero; 5]);
        assert_eq!(summary.groups()[3].brands[0].totals, [zero; 5]);
    }

    #[test]
    fn zero_amount_ratios_are_zero() {
        let zero = MetricRow::default();
        let metrics = SummaryMetrics {
            occur: zero,
            paid: zero,
            unpaid: zero,
            pass: zero,
            wait: zero,
            fail: zero,
            unup: zero,
        };
        for row in metrics.amount_ratios() {
            assert_eq!(
                row,
                (Decimal::ZERO, 0.0, Decimal::ZERO, 0.0, Decimal::ZERO, 0.0,)
            );
        }
    }

    #[test]
    fn anomaly_datasets_extract_orders_and_groups_sections() {
        let app_rows = SheetData::new(vec![
            vec![Data::String("状态".into()), Data::String("发票号码".into())],
            vec![Data::String("审核失败".into()), Data::String("INV2".into())],
            vec![Data::String("已回款".into()), Data::String("INV1".into())],
            vec![Data::String("审核失败".into()), Data::String("INV1".into())],
            vec![
                Data::String("审核失败".into()),
                Data::String("INV_NONE".into()),
            ],
        ]);
        let dig_rows = SheetData::new(vec![
            vec![
                Data::String("状态".into()),
                Data::String("IMEI1".into()),
                Data::String("IMEI2".into()),
            ],
            vec![
                Data::String("审核失败".into()),
                Data::String("EM1".into()),
                Data::String("EM2".into()),
            ],
            vec![
                Data::String("审核通过".into()),
                Data::String("EM3".into()),
                Data::String("EM4".into()),
            ],
            vec![
                Data::String("审核失败".into()),
                Data::String("EM5".into()),
                Data::String("EM6".into()),
            ],
        ]);
        let invoice_rows = SheetData::new(vec![
            vec![Data::String("匹配单据号".into())],
            vec![Data::String("DOC2".into())],
            vec![Data::String("DOC1".into())],
            vec![Data::String("".into())],
            vec![Data::String("DOC3".into())],
        ]);
        let refund_rows = SheetData::new(vec![
            vec![
                Data::String("核销商编".into()),
                Data::String("交易参考号".into()),
            ],
            vec![Data::String("S1".into()), Data::String("R_SINGLE".into())],
            vec![Data::String("S1".into()), Data::String("R_DUP".into())],
            vec![Data::String("S1".into()), Data::String("R_DUP".into())],
            vec![Data::String("S1".into()), Data::String("".into())],
            vec![Data::String("OTHER".into()), Data::String("R_DUP".into())],
        ]);

        let colors = AnomalyColors {
            app_pink: HashSet::from([2, 3, 4, 5]),
            dig_pink: HashSet::new(),
            invoice_yellow: HashSet::from([2, 3, 4, 5]),
            invoice_pink: HashSet::from([5]), // Row 5 is pink -> excluded from yellow
        };

        let mut names = UniqueIndex::default();
        names.insert("INV1".into(), "海尔冰箱".into());
        names.insert("INV2".into(), "美的空调".into());

        let datasets = AnomalyDatasets::extract(
            [
                &app_rows,
                &dig_rows,
                &invoice_rows,
                &refund_rows,
                &refund_rows,
            ],
            &colors,
            ["S1", "S1"],
            &names,
        )
        .unwrap();

        // 1. App failed sorted by product name (INV1 "海尔冰箱" < INV2 "美的空调" < empty)
        assert_eq!(datasets.app_failed.len(), 3);
        assert_eq!(datasets.app_failed[0].extra1, "海尔冰箱");
        assert_eq!(datasets.app_failed[1].extra1, "美的空调");
        assert_eq!(datasets.app_failed[2].extra1, "");

        // 2. Dig failed extracts IMEI1 and IMEI2 in original order
        assert_eq!(datasets.dig_failed.len(), 2);
        assert_eq!(datasets.dig_failed[0].extra1, "EM1");
        assert_eq!(datasets.dig_failed[0].extra2, Some("EM2".into()));
        assert_eq!(datasets.dig_failed[1].extra1, "EM5");
        assert_eq!(datasets.dig_failed[1].extra2, Some("EM6".into()));

        // 3. Invoices sorted by doc_no ascending with empty last (DOC1 < DOC2 < empty)
        assert_eq!(datasets.invoices.rows.len(), 3);
        assert_eq!(cell_to_string(&datasets.invoices.rows[0][0]), "DOC1");
        assert_eq!(cell_to_string(&datasets.invoices.rows[1][0]), "DOC2");
        assert_eq!(cell_to_string(&datasets.invoices.rows[2][0]), "");

        // 4. Refund grouped: duplicates first (R_DUP, R_DUP), singletons second (R_SINGLE), empty last
        assert_eq!(datasets.app_refund.rows.len(), 4);
        assert_eq!(cell_to_string(&datasets.app_refund.rows[0][1]), "R_DUP");
        assert_eq!(cell_to_string(&datasets.app_refund.rows[1][1]), "R_DUP");
        assert_eq!(cell_to_string(&datasets.app_refund.rows[2][1]), "R_SINGLE");
        assert_eq!(cell_to_string(&datasets.app_refund.rows[3][1]), "");
    }

    #[test]
    fn build_invoice_name_index_filters_out_conflicting_names_and_warns() {
        use calamine::Data;
        let rows = vec![
            vec![
                Data::String("数电发票号码".into()),
                Data::String("主要商品名称".into()),
            ],
            vec![Data::String("INV1".into()), Data::String("商品A".into())],
            vec![Data::String("INV1".into()), Data::String("商品B".into())],
            vec![Data::String("INV2".into()), Data::String("商品C".into())],
            vec![Data::String("INV2".into()), Data::String("商品C".into())],
        ];
        let invoices = SheetData::new(rows);
        let index = build_invoice_name_index(&invoices).unwrap();
        assert_eq!(index.get("INV1"), None);
        assert_eq!(index.get("INV2"), Some(&"商品C".to_string()));
        assert_eq!(
            index.warning("发票商品名称索引").as_deref(),
            Some("发票商品名称索引：1 个键对应多条内容不同的记录，已按未命中处理（INV1）")
        );
    }
}
