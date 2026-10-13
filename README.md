# subsidy

Rust 国补数据处理工具：输入原始 Excel 文件夹路径，自动完成8类清洗并生成报表。

当前处理未来城店：数码商户号 `89813014812B04P`，家电电脑商户号 `89813015722APTB`。

```bash
cargo run --locked
```

原始文件只读；明细与报表分别写入原始目录同级的`国补明细/`和`国补报表/`。

- [项目架构与运行验证](docs/architecture.md)
- [清洗业务规则](docs/cleaning-rules.md)
- [清洗模块架构](docs/cleaning-architecture.md)
- [国补报表业务规则](docs/reporting-rules.md)
