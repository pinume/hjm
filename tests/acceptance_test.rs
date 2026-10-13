use calamine::{Reader, open_workbook_auto};
use std::io::{Read, Write};
use std::process::{Command, Stdio};
use subsidy::{generate_store_finance_workbook, generate_summary_workbook};

#[path = "fixtures/acceptance_checks.rs"]
mod acceptance_checks;

#[test]
#[ignore = "requires a current source dataset via SUBSIDY_SOURCE_DIR/COMBINE_SOURCE_DIR or ../data"]
fn test_current_source_acceptance() {
    let source_dir = std::env::var_os("SUBSIDY_SOURCE_DIR")
        .or_else(|| std::env::var_os("COMBINE_SOURCE_DIR"))
        .map(std::path::PathBuf::from)
        .or_else(|| {
            let candidate = std::path::PathBuf::from("../data");
            if candidate.is_dir() {
                Some(candidate)
            } else {
                let candidate = std::path::PathBuf::from("data");
                if candidate.is_dir() {
                    Some(candidate)
                } else {
                    None
                }
            }
        })
        .expect(
            "set SUBSIDY_SOURCE_DIR/COMBINE_SOURCE_DIR or ensure ../data contains the raw dataset",
        );
    let root = std::env::temp_dir().join(format!(
        "subsidy-acceptance-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let input = root.join("原始 数据");
    std::fs::create_dir_all(&input).unwrap();

    let mut copied_sources = Vec::new();
    for entry in std::fs::read_dir(&source_dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_file()
            && path
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("xlsx"))
        {
            let bytes = std::fs::read(&path).unwrap();
            let copied_path = input.join(path.file_name().unwrap());
            std::fs::write(&copied_path, &bytes).unwrap();
            copied_sources.push((path, copied_path, bytes));
        }
    }
    assert!(
        !copied_sources.is_empty(),
        "source directory has no XLSX files"
    );

    let result = run_cli(&root);
    let log = String::from_utf8_lossy(&result.stdout);
    assert!(
        result.status.success(),
        "{log}\n{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(log.matches("处理成功").count(), 8, "{log}");
    assert_eq!(log.matches("报表成功").count(), 2, "{log}");
    assert!(!log.contains("请输入选项"));

    let cleaned = root.join("国补明细");
    for name in [
        "发票明细.xlsx",
        "已上传数码.xlsx",
        "已上传家电电脑.xlsx",
        "银联交易明细门店.xlsx",
        "回款明细家电电脑.xlsx",
        "回款明细数码.xlsx",
        "收款单统计.xlsx",
        "销售用券情况统计.xlsx",
    ] {
        let mut workbook = open_workbook_auto(cleaned.join(name)).unwrap();
        assert_eq!(workbook.sheet_names().len(), 1, "{name}");
        let rows = workbook.worksheet_range_at(0).unwrap().unwrap();
        assert!(rows.height() > 1, "{name} has no data rows");
        assert!(rows.width() > 0, "{name} has no columns");
    }

    let out_dir = root.join("国补报表");
    let summary_path = out_dir.join("国补上传情况汇总.xlsx");
    let finance_path = out_dir.join("26年国补门店财务统筹表.xlsx");
    let summary = open_workbook_auto(&summary_path).unwrap();
    assert_eq!(
        summary.sheet_names(),
        vec![
            "汇总",
            "品类品牌汇总",
            "审核失败明细",
            "异常回款明细",
            "异常发票明细"
        ]
    );
    let finance = open_workbook_auto(&finance_path).unwrap();
    assert_eq!(
        finance.sheet_names(),
        vec![
            "最终匹配表（全部的国补发生数据上匹配）",
            "1.门店国补发生表（银联系统直接导出，不需要加工）",
            "2.门店累计回款表（事业部财务下发，门店筛选自己的）",
            "3.门店上传明细（从门店银联后台每月导出后汇总）"
        ]
    );

    // §3–10 cleaning and §4 reporting: verify business results against this input,
    // before checking that the two report entrypoints agree with each other.
    acceptance_checks::check(&input, &cleaned, &summary_path, &finance_path);

    // Standalone APIs must produce the same workbook contents as the CLI.
    let standalone = root.join("standalone");
    std::fs::create_dir(&standalone).unwrap();
    let standalone_summary = standalone.join("summary.xlsx");
    let standalone_finance = standalone.join("finance.xlsx");
    generate_summary_workbook(&cleaned, &standalone_summary).unwrap();
    generate_store_finance_workbook(&cleaned, &standalone_finance).unwrap();
    assert_workbook_equal(&summary_path, &standalone_summary);
    assert_workbook_equal(&finance_path, &standalone_finance);

    for (source, copied, bytes) in copied_sources {
        assert_eq!(
            std::fs::read(&source).unwrap(),
            bytes,
            "source changed: {}",
            source.display()
        );
        assert_eq!(
            std::fs::read(&copied).unwrap(),
            bytes,
            "copy changed: {}",
            copied.display()
        );
    }

    std::fs::remove_dir_all(root).unwrap();
}

fn run_cli(root: &std::path::Path) -> std::process::Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_subsidy"))
        .current_dir(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    writeln!(child.stdin.take().unwrap(), "  \"原始 数据\"  ").unwrap();
    child.wait_with_output().unwrap()
}

fn assert_workbook_equal(left: &std::path::Path, right: &std::path::Path) {
    let mut left = zip::ZipArchive::new(std::fs::File::open(left).unwrap()).unwrap();
    let mut right = zip::ZipArchive::new(std::fs::File::open(right).unwrap()).unwrap();
    let names: Vec<_> = left.file_names().map(str::to_owned).collect();
    assert_eq!(left.len(), right.len());
    for name in names {
        // Creation timestamps do not affect workbook values or formatting.
        if name == "docProps/core.xml" {
            continue;
        }
        let mut a = Vec::new();
        let mut b = Vec::new();
        left.by_name(&name).unwrap().read_to_end(&mut a).unwrap();
        right.by_name(&name).unwrap().read_to_end(&mut b).unwrap();
        assert!(a == b, "workbook entry differs: {name}");
    }
}
