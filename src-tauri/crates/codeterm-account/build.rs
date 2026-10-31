#[path = "src/build_guard.rs"]
mod build_guard;

fn main() {
    println!("cargo:rerun-if-env-changed=PROFILE");
    let mut values = build_guard::VARIABLES.map(|name| {
        println!("cargo:rerun-if-env-changed={name}");
        std::env::var(name).ok()
    });
    build_guard::resolve_auth_domain(&mut values);
    build_guard::validate(std::env::var("PROFILE").as_deref() != Ok("debug"), &values)
        .unwrap_or_else(|message| panic!("{message}"));
    let fingerprint = build_guard::fingerprint(&values);
    println!(
        "cargo:rustc-env=DOMIOS_FIREBASE_AUTH_DOMAIN={}",
        values[1].as_deref().unwrap_or_default()
    );
    println!("cargo:rustc-check-cfg=cfg(domios_firebase_fingerprint, values(any()))");
    println!("cargo:rustc-cfg=domios_firebase_fingerprint=\"{fingerprint}\"");
    println!("cargo:rustc-env=DOMIOS_FIREBASE_FINGERPRINT={fingerprint}");
}
