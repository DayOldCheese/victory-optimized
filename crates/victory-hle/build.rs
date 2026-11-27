//! The `dlss` feature links NVIDIA's NGX loader (`nvsdk_ngx_d.lib`) from the
//! DLSS SDK named by `VICTORY_DLSS_SDK` (a checkout of NVIDIA/DLSS). The SDK
//! is not part of this repository and its licence keeps it out of it: a build
//! with it is for the machine it was made on, not for distribution.
//!
//! Without the SDK the feature builds a stub (`cfg(ngx)` unset) whose
//! `VICTORY_UPSCALE=dlss` says so, so that `--all-features` builds and tests
//! anywhere (CI has no SDK).

fn main() {
    println!("cargo::rustc-check-cfg=cfg(ngx)");
    println!("cargo::rerun-if-env-changed=VICTORY_DLSS_SDK");
    if std::env::var_os("CARGO_FEATURE_DLSS").is_none() {
        return;
    }
    let Some(sdk) = std::env::var_os("VICTORY_DLSS_SDK").map(std::path::PathBuf::from) else {
        println!("cargo::warning=the dlss feature without VICTORY_DLSS_SDK: DLSS is a stub in this build");
        return;
    };
    let lib = sdk.join("lib").join("Windows_x86_64").join("x64");
    if !lib.join("nvsdk_ngx_d.lib").is_file() {
        println!(
            "cargo::warning=VICTORY_DLSS_SDK={}: no lib/Windows_x86_64/x64/nvsdk_ngx_d.lib there: DLSS is a stub in this build",
            sdk.display()
        );
        return;
    }
    println!("cargo::rerun-if-changed={}", lib.join("nvsdk_ngx_d.lib").display());
    println!("cargo::rustc-link-search=native={}", lib.display());
    println!("cargo::rustc-link-lib=static=nvsdk_ngx_d");
    println!("cargo::rustc-cfg=ngx");
}
