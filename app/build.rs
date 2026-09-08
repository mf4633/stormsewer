// Embed the application icon into the Windows executable (taskbar, Explorer,
// Apps & Features). Other platforms get their icon from the bundle/AppImage.
fn main() {
    println!("cargo:rerun-if-changed=../assets/icon/stormsewer.ico");
    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../assets/icon/stormsewer.ico");
        res.set("ProductName", "StormSewer");
        res.set("FileDescription", "StormSewer storm sewer design and analysis");
        if let Err(e) = res.compile() {
            println!("cargo:warning=icon resource not embedded: {e}");
        }
    }
}
