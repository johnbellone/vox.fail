// On Windows, gives block_avatar.exe its icon (the repository's assets/icon.ico).
fn main() {
    println!("cargo:rerun-if-changed=../../assets/icon.ico");
    #[cfg(windows)]
    {
        if std::path::Path::new("../../assets/icon.ico").exists() {
            let mut res = winresource::WindowsResource::new();
            res.set_icon("../../assets/icon.ico");
            res.set("ProductName", "Block Avatar");
            if let Err(e) = res.compile() {
                println!("cargo:warning=icon not embedded: {e}");
            }
        }
    }
}
