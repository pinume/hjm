use std::collections::HashSet;
use std::path::Path;
use std::rc::Rc;

use super::refund::{self, RefundSubsidyIndices};
use super::uploaded::{self, UploadedStatusIndex};
use super::{MatchStats, MultiValueIndex, SubsidyGroup, invoice, receipts, unionpay};
use crate::model::{ProcessError, Table, Value};

#[derive(Default, Debug)]
struct GroupInputs {
    uploaded: Option<Rc<UploadedStatusIndex>>,
    refund: Option<Rc<RefundSubsidyIndices>>,
    pending_refund: Option<Table>,
    pending_uploaded: Option<Table>,
}

#[derive(Default, Debug)]
pub struct PipelineContext {
    authority: Option<Rc<HashSet<String>>>,
    invoices: Option<Rc<MultiValueIndex>>,
    receipts: Option<Rc<MultiValueIndex>>,
    appliance: GroupInputs,
    digital: GroupInputs,
    stats: Option<MatchStats>,
}

impl PipelineContext {
    pub fn new() -> Self {
        Self::default()
    }

    fn group(&self, group: SubsidyGroup) -> &GroupInputs {
        match group {
            SubsidyGroup::Appliance => &self.appliance,
            SubsidyGroup::Digital => &self.digital,
        }
    }

    fn group_mut(&mut self, group: SubsidyGroup) -> &mut GroupInputs {
        match group {
            SubsidyGroup::Appliance => &mut self.appliance,
            SubsidyGroup::Digital => &mut self.digital,
        }
    }

    pub(crate) fn refund_subsidy_indices(
        &mut self,
        group: SubsidyGroup,
        input_dir: &Path,
    ) -> Result<Rc<RefundSubsidyIndices>, ProcessError> {
        if let Some(index) = &self.group(group).refund {
            return Ok(Rc::clone(index));
        }
        let table = refund::load_refund_table(group, input_dir)?;
        let index = Rc::new(RefundSubsidyIndices::from_table(&table));
        let cached = self.group_mut(group);
        cached.refund = Some(Rc::clone(&index));
        cached.pending_refund = Some(table);
        Ok(index)
    }

    pub(crate) fn store_refund_indices(
        &mut self,
        group: SubsidyGroup,
        indices: RefundSubsidyIndices,
    ) {
        self.group_mut(group).refund = Some(Rc::new(indices));
    }

    pub(crate) fn take_refund_table(&mut self, group: SubsidyGroup) -> Option<Table> {
        self.group_mut(group).pending_refund.take()
    }

    pub(crate) fn uploaded_status_index(
        &mut self,
        group: SubsidyGroup,
        input_dir: &Path,
    ) -> Result<Rc<UploadedStatusIndex>, ProcessError> {
        if let Some(index) = &self.group(group).uploaded {
            return Ok(Rc::clone(index));
        }
        let table = uploaded::load_uploaded_table(group, input_dir, self)?;
        let index = Rc::new(UploadedStatusIndex::from_table(&table));
        let cached = self.group_mut(group);
        cached.uploaded = Some(Rc::clone(&index));
        cached.pending_uploaded = Some(table);
        Ok(index)
    }

    pub(crate) fn store_uploaded_index(&mut self, group: SubsidyGroup, index: UploadedStatusIndex) {
        self.group_mut(group).uploaded = Some(Rc::new(index));
    }

    pub(crate) fn take_uploaded_table(&mut self, group: SubsidyGroup) -> Option<Table> {
        self.group_mut(group).pending_uploaded.take()
    }

    pub(crate) fn authority(
        &mut self,
        input_dir: &Path,
    ) -> Result<Rc<HashSet<String>>, ProcessError> {
        if let Some(index) = &self.authority {
            return Ok(Rc::clone(index));
        }
        let records = unionpay::load_records(input_dir)?;
        let index = Self::build_authority_index(&records);
        self.authority = Some(Rc::clone(&index));
        Ok(index)
    }

    pub(crate) fn store_authority(&mut self, records: &[unionpay::UnionPayRecord]) {
        let index = Self::build_authority_index(records);
        self.authority = Some(index);
    }

    fn build_authority_index(records: &[unionpay::UnionPayRecord]) -> Rc<HashSet<String>> {
        Rc::new(
            records
                .iter()
                .map(|r| r.retrieval_no.as_str())
                .filter(|v| is_valid_ref_no(v))
                .map(str::to_owned)
                .collect(),
        )
    }

    /// 按`匹配单据号`汇总发票明细全部非空`数电发票号码`（未去重）；歧义判定见`coupons::to_row`
    /// 中复用的`resolve`（与 10.6.2 节"命中权威值需唯一"同一原则，不得任选）。
    pub(crate) fn invoice_index(
        &mut self,
        input_dir: &Path,
    ) -> Result<Rc<MultiValueIndex>, ProcessError> {
        if let Some(index) = &self.invoices {
            return Ok(Rc::clone(index));
        }
        let records = invoice::load_records(input_dir)?;
        let index = Self::build_invoice_index(&records);
        self.invoices = Some(Rc::clone(&index));
        Ok(index)
    }

    pub(crate) fn store_invoices(&mut self, records: &[invoice::InvoiceRecord]) {
        let index = Self::build_invoice_index(records);
        self.invoices = Some(index);
    }

    fn build_invoice_index(records: &[invoice::InvoiceRecord]) -> Rc<MultiValueIndex> {
        let mut grouped = MultiValueIndex::new();
        for record in records {
            let Value::Text(match_doc_no) = &record.match_doc_no else {
                continue;
            };
            if record.invoice_no.is_empty() {
                continue;
            }
            grouped.insert(match_doc_no.clone(), record.invoice_no.clone());
        }
        Rc::new(grouped)
    }

    /// 按`匹配单据号`汇总收款单统计全部非空`备注`（未去重）；歧义判定同样复用`resolve`。
    pub(crate) fn receipts_index(
        &mut self,
        input_dir: &Path,
    ) -> Result<Rc<MultiValueIndex>, ProcessError> {
        if let Some(index) = &self.receipts {
            return Ok(Rc::clone(index));
        }
        let records = receipts::load_records(input_dir)?;
        let index = Self::build_receipts_index(&records);
        self.receipts = Some(Rc::clone(&index));
        Ok(index)
    }

    pub(crate) fn store_receipts(&mut self, records: &[receipts::ReceiptRecord]) {
        let index = Self::build_receipts_index(records);
        self.receipts = Some(index);
    }

    fn build_receipts_index(records: &[receipts::ReceiptRecord]) -> Rc<MultiValueIndex> {
        let mut grouped = MultiValueIndex::new();
        for record in records {
            if record.match_doc_no.is_empty() || record.remark.is_empty() {
                continue;
            }
            grouped.insert(record.match_doc_no.clone(), record.remark.clone());
        }
        Rc::new(grouped)
    }

    pub fn record_stats(&mut self, stats: MatchStats) {
        self.stats = Some(stats);
    }

    #[cfg(test)]
    pub fn stats(&self) -> Option<MatchStats> {
        self.stats
    }

    pub fn print_stats(&self) {
        if let Some(stats) = self.stats {
            println!(
                "匹配统计：收款单备注 {}，上传参考号 {}，上传发票号 {}，未上传 {}；合计 {}。",
                stats.hits.receipts,
                stats.hits.uploaded_reference,
                stats.hits.uploaded_invoice,
                stats.hits.unuploaded,
                stats.hits.total()
            );
            println!(
                "歧义记录：参考号提取 {}，发票号码 {}，收款单备注 {}，上传状态 {}（不同阶段可重复计数）。",
                stats.ambiguous.reference,
                stats.ambiguous.invoice,
                stats.ambiguous.receipts,
                stats.ambiguous.uploaded
            );
            println!(
                "未上传原因：关联键为空 {}，候选冲突 {}，查无非空状态 {}；合计 {}（冲突优先归类）。",
                stats.unuploaded.empty_keys,
                stats.unuploaded.candidate_conflict,
                stats.unuploaded.no_status,
                stats.unuploaded.total()
            );
        }
    }
}

fn is_valid_ref_no(value: &str) -> bool {
    value.len() == 12
        && value.as_bytes()[11] == b'N'
        && value.as_bytes()[..11].iter().all(u8::is_ascii_digit)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_and_retrieves_match_stats() {
        let mut ctx = PipelineContext::new();
        assert!(ctx.stats().is_none());
        let stats = MatchStats::default();
        ctx.record_stats(stats);
        assert_eq!(ctx.stats(), Some(stats));
    }

    #[test]
    fn validates_reference_number_shape() {
        assert!(is_valid_ref_no("16867252734N"));
        assert!(!is_valid_ref_no("16867252734W")); // 结尾不是大写 N
        assert!(!is_valid_ref_no("1686725273N")); // 只有 10 位数字
        assert!(!is_valid_ref_no("16867252734n")); // 小写 n 不算
    }
}
