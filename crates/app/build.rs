//! On Windows, put the icon and the version information into the executable.

fn main() {
    #[cfg(windows)]
    {
        let mut resource = winresource::WindowsResource::new();
        resource.set_icon("../ui/assets/icon.ico");
        resource.set("ProductName", "plotextract");
        resource.set("LegalCopyright", "Copyright (c) 2026 Rajat Mandal");
        resource.set("FileDescription", "Pick curves from plots and export them as data");
        if let Err(e) = resource.compile() {
            // a build without the Windows resource compiler still works, just without the icon
            println!("cargo::warning=could not embed the Windows icon: {e}");
        }
    }
    println!("cargo::rerun-if-changed=../ui/assets/icon.ico");
    println!("cargo::rerun-if-changed=build.rs");
}
