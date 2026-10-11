use std::collections::HashMap;

/// 按业务键建立的唯一索引：同一个键的多条记录内容不同时，该键视为冲突，
/// 查询不返回任何一条（不猜测）；内容完全一致的重复仍视为同一条记录。
#[derive(Debug)]
pub(crate) struct UniqueIndex<V> {
    // None 标记冲突键：此后即使再出现内容一致的记录也保持冲突，结果与行顺序无关。
    entries: HashMap<String, Option<V>>,
}

impl<V> Default for UniqueIndex<V> {
    fn default() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }
}

impl<V: PartialEq> UniqueIndex<V> {
    pub(crate) fn insert(&mut self, key: String, value: V) {
        self.entries
            .entry(key)
            .and_modify(|candidate| {
                if candidate.as_ref() != Some(&value) {
                    *candidate = None;
                }
            })
            .or_insert(Some(value));
    }

    pub(crate) fn get(&self, key: &str) -> Option<&V> {
        self.entries.get(key).and_then(Option::as_ref)
    }

    pub(crate) fn conflicts(&self) -> Vec<&str> {
        let mut keys: Vec<_> = self
            .entries
            .iter()
            .filter(|(_, value)| value.is_none())
            .map(|(key, _)| key.as_str())
            .collect();
        keys.sort_unstable();
        keys
    }

    pub(crate) fn warning(&self, label: &str) -> Option<String> {
        let conflicts = self.conflicts();
        if conflicts.is_empty() {
            return None;
        }
        let count = conflicts.len();
        let samples = if count <= 5 {
            conflicts.join("、")
        } else {
            format!("前5个：{}", conflicts[..5].join("、"))
        };
        Some(format!(
            "{label}：{count} 个键对应多条内容不同的记录，已按未命中处理（{samples}）"
        ))
    }

    pub(crate) fn to_valid_map(&self) -> HashMap<String, V>
    where
        V: Clone,
    {
        self.entries
            .iter()
            .filter_map(|(k, v)| v.as_ref().map(|val| (k.clone(), val.clone())))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn differing_duplicates_make_the_key_a_conflict() {
        let mut index = UniqueIndex::default();
        index.insert("REF001".to_string(), "已回款");
        index.insert("REF001".to_string(), "待审核");
        index.insert("REF002".to_string(), "已回款");

        assert_eq!(index.get("REF001"), None);
        assert_eq!(index.get("REF002"), Some(&"已回款"));
        assert_eq!(index.conflicts(), vec!["REF001"]);
    }

    #[test]
    fn identical_duplicates_are_not_conflicts() {
        let mut index = UniqueIndex::default();
        index.insert("REF001".to_string(), "已回款");
        index.insert("REF001".to_string(), "已回款");

        assert_eq!(index.get("REF001"), Some(&"已回款"));
        assert!(index.conflicts().is_empty());
    }

    #[test]
    fn conflicts_do_not_depend_on_row_order() {
        let rows = [
            ("REF001", "已回款"),
            ("REF001", "待审核"),
            ("REF002", "未上传"),
        ];
        let mut forward = UniqueIndex::default();
        let mut backward = UniqueIndex::default();
        for (key, value) in rows {
            forward.insert(key.to_string(), value);
        }
        for (key, value) in rows.into_iter().rev() {
            backward.insert(key.to_string(), value);
        }

        assert_eq!(forward.get("REF001"), None);
        assert_eq!(backward.get("REF001"), None);
        assert_eq!(forward.get("REF002"), backward.get("REF002"));
        assert_eq!(forward.conflicts(), backward.conflicts());
    }

    #[test]
    fn a_later_identical_row_does_not_clear_a_conflict() {
        let mut index = UniqueIndex::default();
        index.insert("REF001".to_string(), "已回款");
        index.insert("REF001".to_string(), "待审核");
        index.insert("REF001".to_string(), "已回款");

        assert_eq!(index.get("REF001"), None);
        assert_eq!(index.conflicts(), vec!["REF001"]);
    }

    #[test]
    fn warning_names_the_index_counts_conflicts_and_lists_up_to_five_keys() {
        let mut index = UniqueIndex::default();
        assert_eq!(index.warning("上传索引"), None);

        for key in ["K1", "K2", "K3"] {
            index.insert(key.to_string(), 1);
            index.insert(key.to_string(), 2);
        }
        assert_eq!(
            index.warning("上传索引").as_deref(),
            Some("上传索引：3 个键对应多条内容不同的记录，已按未命中处理（K1、K2、K3）")
        );

        for key in ["K4", "K5", "K6", "K7"] {
            index.insert(key.to_string(), 1);
            index.insert(key.to_string(), 2);
        }
        assert_eq!(
            index.warning("上传索引").as_deref(),
            Some(
                "上传索引：7 个键对应多条内容不同的记录，已按未命中处理（前5个：K1、K2、K3、K4、K5）"
            )
        );
    }

    #[test]
    fn to_valid_map_filters_out_conflicts() {
        let mut index = UniqueIndex::default();
        index.insert("VALID".to_string(), "val");
        index.insert("CONFLICT".to_string(), "a");
        index.insert("CONFLICT".to_string(), "b");

        let map = index.to_valid_map();
        assert_eq!(map.get("VALID"), Some(&"val"));
        assert_eq!(map.get("CONFLICT"), None);
    }
}
