# HJM (哈基米)

<p align="center">
  <img src="docs/assets/hjm_icon.png" width="160" alt="HJM App Icon" />
</p>

Rust 国补数据处理工具（HJM）：输入原始 Excel 文件夹路径，自动完成8类清洗并生成报表。

当前处理未来城店：数码商户号 `89813014812B04P`，家电电脑商户号 `89813015722APTB`。

```bash
cargo run --release --locked
```

原始文件只读；明细与报表分别写入原始目录同级的`国补明细/`和`国补报表/`。

Windows 下使用带 Windows SDK 的 Visual Studio C++ Build Tools，执行
`cargo build --release --locked`，生成的 `target/release/hjm.exe` 会自动嵌入 HJM 图标，无需额外携带图标文件。

- [项目架构与运行验证](docs/architecture.md)
- [清洗业务规则](docs/cleaning-rules.md)
- [清洗模块架构](docs/cleaning-architecture.md)
- [国补报表业务规则](docs/reporting-rules.md)
