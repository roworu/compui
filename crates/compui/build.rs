//! on macos: compiles the swift host (`macos/`) into a static library linked into `compui`, so
//! `compui host` runs it from the one binary
use std::{env, fs, path::PathBuf, process::Command};

fn main() {
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }
    let root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("../../macos");
    println!("cargo:rerun-if-changed={}", root.display());
    let mut srcs = Vec::new();
    for dir in fs::read_dir(&root).unwrap().map_while(Result::ok) {
        for f in fs::read_dir(dir.path()).unwrap().map_while(Result::ok) {
            if f.path().extension().is_some_and(|e| e == "swift") {
                srcs.push(f.path());
            }
        }
    }
    let arch = match env::var("CARGO_CFG_TARGET_ARCH").unwrap().as_str() {
        "aarch64" => "arm64".to_string(),
        a => a.to_string(),
    };
    let target = format!("{arch}-apple-macosx{}", env::var("MACOSX_DEPLOYMENT_TARGET").unwrap_or("11.0".into()));
    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    let ok = Command::new("swiftc")
        .args(["-O", "-parse-as-library", "-emit-library", "-static", "-module-name", "compuihost", "-target", &target, "-o"])
        .arg(out.join("libcompuihost.a"))
        .args(&srcs)
        .status()
        .expect("swiftc")
        .success();
    assert!(ok, "swiftc failed");
    // the swift runtime the library's autolink entries name
    let info = Command::new("swiftc").args(["-target", &target, "-print-target-info"]).output().expect("swiftc");
    let info: serde_json::Value = serde_json::from_slice(&info.stdout).expect("swiftc target info");
    for p in info["paths"]["runtimeLibraryPaths"].as_array().into_iter().flatten().filter_map(|p| p.as_str()) {
        println!("cargo:rustc-link-search=native={p}");
    }
    println!("cargo:rustc-link-search=native={}", out.display());
    println!("cargo:rustc-link-lib=static=compuihost");
    println!("cargo:rustc-link-lib=framework=Cocoa");
    println!("cargo:rustc-link-lib=framework=ApplicationServices");
    println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
}
