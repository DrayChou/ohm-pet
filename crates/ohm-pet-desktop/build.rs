fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows")
        || !std::env::var("HOST").is_ok_and(|host| host.contains("windows"))
    {
        return;
    }

    let icon =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packaging/windows/OHMPet.ico");
    let mut resource = winres::WindowsResource::new();
    resource.set_icon(icon.to_string_lossy().as_ref());
    resource.set("ProductName", "OHM Pet");
    resource.set("FileDescription", "OHM Pet desktop companion");
    resource.set("LegalCopyright", "OHM Pet contributors");
    resource.compile().expect("compile Windows resources");
}
