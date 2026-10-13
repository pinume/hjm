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

    /// 第3.3节：将发票商品类别片段映射为财务大类。
    pub(crate) fn invoice_category(token: &str) -> Option<&'static str> {
        Some(match token {
            "国产手机" | "合资手机" | "手机" | "手机串号" | "平板" | "平板电脑" | "学习机"
            | "电话手表" | "手表" | "智能手表" | "手环" | "智能手环" | "智能眼镜" | "智能耳机" => {
                "数码"
            }
            "电脑" => "电脑",
            "空调"
            | "家用挂机直流无氟变频1.5P35"
            | "家用柜机直流无氟变频3P72"
            | "风管机"
            | "中央空调多联机"
            | "家用挂机直流无氟变频2P50"
            | "家用柜机直流无氟变频4P"
            | "家用多联机外机"
            | "家用挂机直流无氟变频1P26"
            | "家用挂机直流无氟变频3P"
            | "家用柜机直流无氟变频5P"
            | "多联空调"
            | "多联内机"
            | "厨房空调器"
            | "厨房空调"
            | "多联机配件（控制器）" => "空调",
            "洗衣机" | "洗鞋一体机" | "干衣机" => "洗衣机",
            "冰箱" | "冷柜" | "冰吧" | "冰柜" => "冰箱",
            "电热水器"
            | "燃气热水器"
            | "欧式烟机"
            | "嵌入式灶"
            | "热水器其他"
            | "欧式烟机CXW"
            | "水系统"
            | "电器"
            | "进吸式烟机"
            | "洗碗机cw"
            | "洗碗机（厨卫）"
            | "岛式烟机"
            | "即热式热水器"
            | "CWS"
            | "嵌入式消毒柜"
            | "嵌入式蒸箱\\烤箱\\微波炉" => "厨卫",
            "彩电"
            | "国产普通LED75寸"
            | "国产普通LED85寸"
            | "国产普通"
            | "国产普通LED100寸"
            | "国产普通LED98寸"
            | "激光投影"
            | "彩电艺术显示"
            | "国产普通LED86寸"
            | "国产激光4K88寸"
            | "国产激光4K80寸"
            | "国产激光LED100寸"
            | "国产激光LED86寸"
            | "国产艺术电视LED75寸"
            | "壁纸电视"
            | "国产壁纸电视LED85寸"
            | "艺术显示彩电"
            | "投影仪"
            | "国产艺术电视LED85寸" => "彩电",
            "净水类"
            | "净水类（小电）"
            | "净水机"
            | "洁净类"
            | "其他生活小电"
            | "电解水机"
            | "锅具"
            | "炉具"
            | "加工机"
            | "智能锁" => "小电",
            _ => return None,
        })
    }

    /// 第3.3节：无半角连字符名称仅按已确认的完整名称映射财务大类。
    pub(crate) fn invoice_category_from_unseparated_name(name: &str) -> Option<&'static str> {
        match name {
            "OPPO手机"
            | "vivo手机"
            | "IQOO手机"
            | "华为手机"
            | "一加手机"
            | "小天才手表"
            | "荣耀手机"
            | "小天才电话手表"
            | "学而思学习机"
            | "作业帮学习机"
            | "华为手表"
            | "小米手机"
            | "手机"
            | "华为平板"
            | "OPPO平板电脑"
            | "平板"
            | "苹果平板电脑"
            | "苹果手机"
            | "荣耀平板电脑"
            | "手表"
            | "一加平板"
            | "红米手机"
            | "OPPO手表"
            | "华为手环"
            | "VIVO手表"
            | "OPPO电话手表"
            | "苹果手表"
            | "小天才学习机"
            | "OPPO智能手表"
            | "家用平板电脑"
            | "小天才"
            | "华为数码"
            | "OPPO手机手机"
            | "VIVO手机"
            | "OPPO 平板"
            | "华为 平板"
            | "荣耀电话手表"
            | "oppo手表"
            | "vivo平板"
            | "IQ手机"
            | "华为智能穿戴"
            | "vivo Y60i 6GB+128GB"
            | "iQ手机"
            | "ViVO手机"
            | "HUAWEI平板电脑"
            | "荣耀手环"
            | "苹果平板"
            | "长城电源"
            | "IQOO手机手机"
            | "VIXO手机"
            | "iqoo手机"
            | "手机vivo"
            | "步步高学习机"
            | "荣耀"
            | "IQ00国产手机"
            | "OPPO数码"
            | "Reno手机" => Some("数码"),
            "海尔空调" | "美的空调" | "卡萨帝空调" | "TCL空调" | "晶弘空调" | "奥克斯空调"
            | "格力空调" | "海尔你空调" | "空调" | "统帅空调" | "美的中央空调" | "COLMO空调" => {
                Some("空调")
            }
            "海尔冰箱" | "美菱冰箱" | "卡萨帝冰箱" | "美的冰箱" | "海尔冷柜" | "西门子电冰箱"
            | "西门子冰箱" | "美菱电冰箱" | "美的冰柜" => Some("冰箱"),
            "海尔洗衣机" | "卡萨帝洗衣机" | "小天鹅洗衣机" | "美菱洗衣机" | "西门子洗衣机"
            | "西门子干衣机" => Some("洗衣机"),
            "海尔热水器"
            | "海尔电热水器"
            | "卡萨帝电热水器"
            | "海尔燃气热水器"
            | "COLMO电热水器"
            | "烟机灶具套餐"
            | "卡萨帝燃气热水器"
            | "欧意欧式烟机"
            | "方太灶具"
            | "COLMO燃气热水器"
            | "卡萨帝热水器"
            | "老板烟机"
            | "老板灶具" => Some("厨卫"),
            "创维电视" | "海信电视" | "TCL电视" | "海尔电视" => Some("彩电"),
            "美的油汀" | "沁园净水类" | "美的小电风扇" | "沁园净水机" => {
                Some("小电")
            }
            _ => None,
        }
    }

    /// 第3.3节：已核对的完整发票商品名称精确匹配，未收录名称沿用原解析规则。
    pub(crate) fn from_invoice_catalog(name: &str) -> Option<Self> {
        use std::collections::HashMap;
        use std::sync::OnceLock;

        static CATALOG: OnceLock<HashMap<&str, ProductIdentity>> = OnceLock::new();
        CATALOG
            .get_or_init(|| {
                include_str!("invoice_products.tsv")
                    .lines()
                    .skip(1)
                    .map(|line| {
                        let mut fields = line.split('\t');
                        let name = fields.next().expect("商品名称");
                        let category = fields.next().expect("财务大类");
                        let brand = fields.next().expect("财务品牌");
                        (
                            name,
                            Self::new(
                                normalize_invoice_brand(brand),
                                normalize_finance_category(category),
                            ),
                        )
                    })
                    .collect()
            })
            .get(name)
            .cloned()
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
    let brand = brand.split(['(', '（']).next().unwrap_or(brand).trim();
    match brand.to_ascii_lowercase().as_str() {
        "vivo" => "vivo".to_string(),
        "iqoo" => "iqoo".to_string(),
        "huawei" => "华为".to_string(),
        "oppo" => "OPPO".to_string(),
        "colmo厨热jx水系统" => "美的".to_string(),
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
    fn invoice_catalog_has_unique_names_and_valid_categories() {
        let mut names = std::collections::HashSet::new();
        for line in include_str!("invoice_products.tsv").lines().skip(1) {
            let fields: Vec<_> = line.split('\t').collect();
            assert_eq!(fields.len(), 6, "{line}");
            assert!(!fields[0].is_empty());
            assert!(names.insert(fields[0]), "重复商品名称：{}", fields[0]);
            assert!(
                [
                    "",
                    "数码",
                    "电脑",
                    "空调",
                    "冰箱",
                    "洗衣机",
                    "厨卫",
                    "彩电",
                    "小电",
                    "其他"
                ]
                .contains(&fields[1]),
                "{line}"
            );
        }
        assert!(!names.is_empty());
    }

    #[test]
    fn invoice_catalog_uses_products_instead_of_mixed_source_categories() {
        for (name, brand, category) in [
            ("XQG120-PBSEU78BU1", "海尔", "洗衣机"),
            ("COLMO洗衣机CGU10H-HS1", "美的", "洗衣机"),
            ("小米（直营店）-生态链-米家冰箱 对开636L", "小米", "冰箱"),
            (
                "联想-电脑周边新-motoG100Pro-12+256-云墨黑",
                "摩托罗拉",
                "数码",
            ),
            ("联想-电脑新-MOTO G100S 8+256G陨黑", "摩托罗拉", "数码"),
            ("苹果-电脑新-IPAD WIFI 256GB BLU-MD4H4CH/A", "苹果", "数码"),
            ("联想-电脑周边新-联想小新Pro13-8+256-B", "联想", "数码"),
            (
                "新产品（北艾）-生态链-猫王-小王子 FM/蓝牙音箱 奶油色 布朗熊特别版 典藏款 MW-2AC",
                "猫王",
                "数码",
            ),
            ("物业管理费", "", ""),
            ("洗衣机", "", "洗衣机"),
            ("华为平板电脑", "华为", "数码"),
        ] {
            assert_eq!(
                ProductIdentity::from_invoice_catalog(name),
                Some(ProductIdentity::new(brand, category)),
                "{name}"
            );
        }
        assert_eq!(
            ProductIdentity::from_invoice_catalog("未核实新品牌-生态链-未知型号"),
            None
        );
    }

    #[test]
    fn invoice_catalog_keeps_oneplus_actual_brand_separate_from_finance_brand() {
        let mut found = false;
        for line in include_str!("invoice_products.tsv").lines().skip(1) {
            let fields: Vec<_> = line.split('\t').collect();
            let parts: Vec<_> = fields[0].splitn(3, '-').collect();
            if parts.len() == 3
                && parts[0].starts_with("OPPO")
                && ["手机", "平板电脑"].contains(&parts[1])
                && parts[2].starts_with("一加")
            {
                found = true;
                assert_eq!(fields[2], "OPPO", "财务口径应保留：{}", fields[0]);
                assert_eq!(fields[3], "一加", "实际商品品牌：{}", fields[0]);
            }
        }
        assert!(found);
    }

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
        assert_eq!(normalize_invoice_brand("VIVO(步步高)"), "vivo");
        assert_eq!(normalize_invoice_brand("VIVO（欢迪颂）"), "vivo");
        assert_eq!(normalize_invoice_brand("OPPO(本德)"), "OPPO");
        assert_eq!(normalize_invoice_brand("华为(盛腾)"), "华为");
        assert_eq!(normalize_invoice_brand("华为（F）"), "华为");
        assert_eq!(normalize_invoice_brand("华为（移动）"), "华为");
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

    #[test]
    fn invoice_category_maps_confirmed_digital_and_computer_categories() {
        for token in [
            "国产手机",
            "合资手机",
            "手机",
            "手机串号",
            "平板",
            "平板电脑",
            "学习机",
            "电话手表",
            "手表",
            "智能手表",
            "手环",
            "智能手环",
            "智能眼镜",
            "智能耳机",
        ] {
            assert_eq!(
                ProductIdentity::invoice_category(token),
                Some("数码"),
                "{token}"
            );
        }
        assert_eq!(ProductIdentity::invoice_category("电脑"), Some("电脑"));
        assert_eq!(ProductIdentity::invoice_category("未核实新类别"), None);
    }

    #[test]
    fn unseparated_invoice_categories_require_an_exact_name() {
        assert_eq!(
            ProductIdentity::invoice_category_from_unseparated_name("HUAWEI平板电脑"),
            Some("数码")
        );
        assert_eq!(
            ProductIdentity::invoice_category_from_unseparated_name("华为新增平板"),
            None
        );
    }
}
