fn main() {
    // The icon + application manifest are Windows PE resources. `embed-resource`
    // is a Windows-only build dependency, so keep the reference behind a cfg too.
    #[cfg(windows)]
    {
        let _ = embed_resource::compile("app.rc", embed_resource::NONE);
        println!("cargo:rerun-if-changed=app.rc");
        println!("cargo:rerun-if-changed=app.manifest");
        println!("cargo:rerun-if-changed=src/assets/app.ico");
    }

    // Tray icons are pulled in via include_bytes!, so rustc already tracks them.
    println!("cargo:rerun-if-changed=src/assets/tray_idle.rgba");
    println!("cargo:rerun-if-changed=src/assets/tray_running.rgba");
    println!("cargo:rerun-if-changed=src/assets/tray_stopped.rgba");
}
