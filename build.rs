fn main() {
    println!("cargo:rerun-if-changed=docs/assets/hjm.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new()
            .set_icon("docs/assets/hjm.ico")
            .set("ProductName", "HJM")
            .set("FileDescription", "HJM")
            .compile()
            .expect("无法编译 Windows 图标资源，请检查 Windows SDK 或 MinGW 资源编译器");
    }
}
