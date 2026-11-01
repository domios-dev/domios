#[path = "src/key_fingerprint.rs"]
mod key_fingerprint;

fn main() {
    for (key_name, fingerprint_name) in [
        (
            "DOMIOS_AMPLITUDE_API_KEY_TEST",
            "DOMIOS_AMPLITUDE_FINGERPRINT_TEST",
        ),
        (
            "DOMIOS_AMPLITUDE_API_KEY_PROD",
            "DOMIOS_AMPLITUDE_FINGERPRINT_PROD",
        ),
    ] {
        println!("cargo:rerun-if-env-changed={key_name}");
        let key = std::env::var(key_name).ok();
        let fingerprint = key_fingerprint::fingerprint(key.as_deref());
        let cfg_name = fingerprint_name.to_ascii_lowercase();
        println!("cargo:rustc-check-cfg=cfg({cfg_name}, values(any()))");
        println!("cargo:rustc-cfg={cfg_name}=\"{fingerprint}\"");
        println!("cargo:rustc-env={fingerprint_name}={fingerprint}");
    }
}
