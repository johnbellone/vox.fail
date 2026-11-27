// On Windows, gives vox-client.exe its icon (the repository's assets/icon.ico) and product name.
// VOX_VERSION (set by the release scripts) is read with option_env! in main.rs; the rerun line
// makes cargo rebuild when it changes.
fn main() {
    println!("cargo:rerun-if-env-changed=VOX_VERSION");
    println!("cargo:rerun-if-changed=../../assets/icon.ico");
    #[cfg(windows)]
    {
        if std::path::Path::new("../../assets/icon.ico").exists() {
            let mut res = winresource::WindowsResource::new();
            res.set_icon("../../assets/icon.ico");
            res.set("ProductName", "Vox");
            if let Err(e) = res.compile() {
                println!("cargo:warning=icon not embedded: {e}");
            }
        }
    }
}
