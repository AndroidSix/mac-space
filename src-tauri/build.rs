fn main() {
    cc::Build::new()
        .file("native/thermal.m")
        .flag("-fobjc-arc")
        .compile("macspace_thermal");
    println!("cargo:rustc-link-lib=framework=Foundation");
    println!("cargo:rustc-link-lib=framework=Security");
    println!("cargo:rustc-link-lib=framework=LocalAuthentication");
    println!("cargo:rerun-if-changed=native/thermal.m");
    tauri_build::build();
}
