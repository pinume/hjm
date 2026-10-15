/// 最终未上传的互斥原因：关联键为空、候选冲突、查无非空状态。
#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnuploadedBreakdown {
    pub empty_keys: usize,
    pub candidate_conflict: usize,
    pub no_status: usize,
}

impl UnuploadedBreakdown {
    pub fn total(&self) -> usize {
        self.empty_keys + self.candidate_conflict + self.no_status
    }
}

#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub struct MatchHits {
    pub receipts: usize,
    pub uploaded_reference: usize,
    pub uploaded_invoice: usize,
    pub unuploaded: usize,
}

impl MatchHits {
    pub fn total(&self) -> usize {
        self.receipts + self.uploaded_reference + self.uploaded_invoice + self.unuploaded
    }
}

#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub struct AmbiguityCounts {
    pub reference: usize,
    pub invoice: usize,
    pub receipts: usize,
    pub uploaded: usize,
}

#[derive(Default, Clone, Copy, Debug)]
pub(crate) struct MatchAmbiguities {
    pub reference: bool,
    pub invoice: bool,
    pub receipts: bool,
    pub uploaded: bool,
}

impl MatchAmbiguities {
    fn any(self) -> bool {
        self.reference || self.invoice || self.receipts || self.uploaded
    }
}

pub(crate) enum MatchOutcome {
    Receipts,
    UploadedReference,
    UploadedInvoice,
    Unuploaded { has_keys: bool },
}

pub(crate) struct RecordMatch {
    pub outcome: MatchOutcome,
    pub ambiguous: MatchAmbiguities,
}

/// 命中分类互斥；不同阶段的歧义可重复计数。
#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub struct MatchStats {
    pub hits: MatchHits,
    pub ambiguous: AmbiguityCounts,
    pub unuploaded: UnuploadedBreakdown,
}

impl MatchStats {
    pub(crate) fn record(&mut self, result: RecordMatch) {
        let ambiguous = result.ambiguous;
        self.ambiguous.reference += usize::from(ambiguous.reference);
        self.ambiguous.invoice += usize::from(ambiguous.invoice);
        self.ambiguous.receipts += usize::from(ambiguous.receipts);
        self.ambiguous.uploaded += usize::from(ambiguous.uploaded);
        match result.outcome {
            MatchOutcome::Receipts => self.hits.receipts += 1,
            MatchOutcome::UploadedReference => self.hits.uploaded_reference += 1,
            MatchOutcome::UploadedInvoice => self.hits.uploaded_invoice += 1,
            MatchOutcome::Unuploaded { has_keys } => {
                self.hits.unuploaded += 1;
                // 仅统计最终未上传；候选冲突优先于关联键为空。
                if ambiguous.any() {
                    self.unuploaded.candidate_conflict += 1;
                } else if !has_keys {
                    self.unuploaded.empty_keys += 1;
                } else {
                    self.unuploaded.no_status += 1;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_exclusive_outcomes_and_prioritizes_conflicts_only_for_final_misses() {
        let mut stats = MatchStats::default();
        for outcome in [
            MatchOutcome::Receipts,
            MatchOutcome::UploadedReference,
            MatchOutcome::UploadedInvoice,
            MatchOutcome::Unuploaded { has_keys: false },
        ] {
            stats.record(RecordMatch {
                outcome,
                ambiguous: MatchAmbiguities {
                    reference: true,
                    invoice: true,
                    receipts: true,
                    uploaded: true,
                },
            });
        }
        for has_keys in [false, true] {
            stats.record(RecordMatch {
                outcome: MatchOutcome::Unuploaded { has_keys },
                ambiguous: MatchAmbiguities::default(),
            });
        }
        assert_eq!(
            stats.hits,
            MatchHits {
                receipts: 1,
                uploaded_reference: 1,
                uploaded_invoice: 1,
                unuploaded: 3,
            }
        );
        assert_eq!(
            stats.ambiguous,
            AmbiguityCounts {
                reference: 4,
                invoice: 4,
                receipts: 4,
                uploaded: 4,
            }
        );
        assert_eq!(
            stats.unuploaded,
            UnuploadedBreakdown {
                empty_keys: 1,
                candidate_conflict: 1,
                no_status: 1,
            }
        );
        assert_eq!(stats.hits.total(), 6);
        assert_eq!(stats.unuploaded.total(), stats.hits.unuploaded);
    }
}
