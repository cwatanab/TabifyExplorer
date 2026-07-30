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

        // Copy app.ico to target directory next to executable
        if let Ok(out_dir) = std::env::var("OUT_DIR") {
            let out_path = std::path::PathBuf::from(out_dir);
            // OUT_DIR is typically target/<profile>/build/<pkg>-<hash>/out
            if let Some(target_dir) = out_path.ancestors().nth(3) {
                let dest = target_dir.join("app.ico");
                let _ = std::fs::copy("app.ico", dest);
            }
        }
    }
}
