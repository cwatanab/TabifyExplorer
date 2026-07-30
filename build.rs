fn main() {
    println!("cargo:rerun-if-changed=app.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default() == "windows" {
        let mut res = winres::WindowsResource::new();
        res.set_icon("app.ico");
        if let Err(e) = res.compile() {
            println!(
                "cargo:warning=Failed to compile Windows resource icon: {}",
                e
            );
        }
    }
}
