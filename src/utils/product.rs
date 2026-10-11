/// 产品标识值对象：封装规范化品牌与规范化财务大类。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ProductIdentity {
    pub brand: String,
    pub category: String,
}

impl ProductIdentity {
    pub fn new(brand: impl Into<String>, category: impl Into<String>) -> Self {
        Self {
            brand: brand.into(),
            category: category.into(),
        }
    }

    /// 销售用券产品标识：执行第 10.11 节品牌和财务大类归并
    pub fn from_sales(brand: &str, category: &str) -> Self {
        Self {
            brand: normalize_sales_brand(brand),
            category: normalize_finance_category(category),
        }
    }
}

/// 10.11 节品牌归并：仅替换列出的原值，其余原样保留。
pub fn normalize_sales_brand(value: &str) -> String {
    match value {
        "COLMO厨热JX" | "美的厨热JX" | "东芝JX" | "华凌" | "小天鹅" | "COLMO" => {
            "美的".to_string()
        }
        "卡萨帝" | "统帅" => "海尔".to_string(),
        "晶弘" => "格力".to_string(),
        _ => value.to_string(),
    }
}

/// 发票品牌归并：处理特殊品牌大小写/子类，并回退到销售用券标准归并。
pub fn normalize_invoice_brand(brand: &str) -> String {
    match brand.to_ascii_lowercase().as_str() {
        "vivo" => "vivo".to_string(),
        "iqoo" => "iqoo".to_string(),
        "huawei" => "华为".to_string(),
        "oppo" => "OPPO".to_string(),
        "美的（小电）" | "美的（微清）" | "colmo厨热jx水系统" => {
            "美的".to_string()
        }
        _ => normalize_sales_brand(brand),
    }
}

/// 10.11 节财务大类归并：仅替换列出的原值，其余原样保留。
pub fn normalize_finance_category(value: &str) -> String {
    match value {
        "国产彩电" | "进口彩电" => "彩电".to_string(),
        "新业务类" => "数码".to_string(),
        _ => value.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_listed_brands_and_keeps_others() {
        for brand in [
            "COLMO厨热JX",
            "美的厨热JX",
            "东芝JX",
            "华凌",
            "小天鹅",
            "COLMO",
        ] {
            assert_eq!(normalize_sales_brand(brand), "美的");
        }
        for brand in ["卡萨帝", "统帅"] {
            assert_eq!(normalize_sales_brand(brand), "海尔");
        }
        assert_eq!(normalize_sales_brand("晶弘"), "格力");
        assert_eq!(normalize_sales_brand("海信"), "海信"); // 未列出的品牌原样保留
    }

    #[test]
    fn normalizes_invoice_brands_with_case_insensitivity_and_fallbacks() {
        assert_eq!(normalize_invoice_brand("VIVO"), "vivo");
        assert_eq!(normalize_invoice_brand("iqoo"), "iqoo");
        assert_eq!(normalize_invoice_brand("HUAWEI"), "华为");
        assert_eq!(normalize_invoice_brand("oppo"), "OPPO");
        assert_eq!(normalize_invoice_brand("美的（小电）"), "美的");
        assert_eq!(normalize_invoice_brand("colmo厨热jx水系统"), "美的");
        // Fallback to sales brand normalization:
        assert_eq!(normalize_invoice_brand("卡萨帝"), "海尔");
        assert_eq!(normalize_invoice_brand("晶弘"), "格力");
        assert_eq!(normalize_invoice_brand("飞利浦"), "飞利浦");
    }

    #[test]
    fn normalizes_listed_finance_categories_and_keeps_others() {
        assert_eq!(normalize_finance_category("国产彩电"), "彩电");
        assert_eq!(normalize_finance_category("进口彩电"), "彩电");
        assert_eq!(normalize_finance_category("新业务类"), "数码");
        assert_eq!(normalize_finance_category("空调"), "空调"); // 未列出的类别原样保留
    }

    #[test]
    fn product_identity_from_sales_normalizes_both_brand_and_category() {
        let identity = ProductIdentity::from_sales("华凌", "国产彩电");
        assert_eq!(identity.brand, "美的");
        assert_eq!(identity.category, "彩电");
    }
}
