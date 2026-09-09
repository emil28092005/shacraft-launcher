fn main() {
    println!("cargo:rerun-if-env-changed=SHACRAFT_UPDATER_PUBLIC_KEY");
    println!("cargo:rerun-if-env-changed=SHACRAFT_UPDATER_TEST_BUILD");
    println!("cargo:rerun-if-changed=updater-public-key.txt");
    tauri_build::build()
}
