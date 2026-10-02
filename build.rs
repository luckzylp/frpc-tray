fn main() {
    let _ = embed_resource::compile("app.rc", embed_resource::NONE);
    println!("cargo:rerun-if-changed=app.rc");
    println!("cargo:rerun-if-changed=app.manifest");
    println!("cargo:rerun-if-changed=src/assets/app.ico");
    println!("cargo:rerun-if-changed=src/assets/tray_idle.rgba");
    println!("cargo:rerun-if-changed=src/assets/tray_running.rgba");
    println!("cargo:rerun-if-changed=src/assets/tray_stopped.rgba");
}
