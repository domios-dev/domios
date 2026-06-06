fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let target_env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if target_env == "msvc" {
        println!("cargo:rustc-link-arg-bin=codeterm=/STACK:8388608");
    } else {
        println!("cargo:rustc-link-arg-bin=codeterm=-Wl,--stack,8388608");
    }
}
