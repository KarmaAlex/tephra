// Replacement build script for the `gpui-pre-windows` crate, used only by the Linux -> Windows
// cross build (packaging/windows/build.sh patches it in with `cargo --config patch.crates-io...`).
//
// Upstream's build.rs precompiles the DirectX HLSL shaders with the Windows SDK's fxc.exe, but
// only `#[cfg(target_os = "windows")]` -- i.e. when the *build host* is Windows. Cross-compiling
// from Linux skips it, and the release build then fails on the missing `shaders_bytes.rs`.
// This version keys off the *target* (CARGO_CFG_TARGET_OS) and compiles the shaders with
// vkd3d-compiler (open-source HLSL -> DXBC, shader model 4.1, same entry points/profiles as fxc).
// Set GPUI_HLSL_COMPILER to use another vkd3d-compiler binary.

use std::{env, fmt::Write as _, fs, path::PathBuf, process::Command};

const MODULES: [&str; 8] = [
    "quad",
    "shadow",
    "path_rasterization",
    "path_sprite",
    "underline",
    "monochrome_sprite",
    "subpixel_sprite",
    "polychrome_sprite",
];

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/shaders.hlsl");
    println!("cargo:rerun-if-changed=src/alpha_correction.hlsl");
    println!("cargo:rerun-if-env-changed=GPUI_HLSL_COMPILER");
    let target_windows = env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows");
    let release = env::var("CARGO_CFG_DEBUG_ASSERTIONS").is_err();
    if !(target_windows && release) {
        return; // debug builds compile the shaders at runtime (D3DCompileFromFile)
    }

    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let src = manifest.join("src/shaders.hlsl");
    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    let compiler = env::var("GPUI_HLSL_COMPILER").unwrap_or_else(|_| "vkd3d-compiler".into());

    let mut rust = String::new();
    for module in MODULES {
        for (stage, profile, suffix) in [("vertex", "vs_4_1", "VERTEX"), ("fragment", "ps_4_1", "FRAGMENT")] {
            let entry = format!("{module}_{stage}");
            let obj = out.join(format!("{entry}.dxbc"));
            let res = Command::new(&compiler)
                .current_dir(manifest.join("src")) // resolves #include "alpha_correction.hlsl"
                .args(["-x", "hlsl", "-b", "dxbc-tpf", "-p", profile, "-e", &entry, "-o"])
                .arg(&obj)
                .arg(&src)
                .output()
                .unwrap_or_else(|e| panic!("cannot run {compiler} (install vkd3d): {e}"));
            if !res.status.success() {
                panic!("{compiler} failed for {entry}:\n{}", String::from_utf8_lossy(&res.stderr));
            }
            let bytes = fs::read(&obj).unwrap();
            let name = format!("{}_{suffix}_BYTES", module.to_uppercase());
            let list = bytes.iter().map(|b| b.to_string()).collect::<Vec<_>>().join(",");
            writeln!(rust, "const {name}: &[u8] = &[{list}];").unwrap();
        }
    }
    fs::write(out.join("shaders_bytes.rs"), rust).unwrap();
}
