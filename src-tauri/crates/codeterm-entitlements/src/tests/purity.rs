const SOURCES: [(&str, &str); 10] = [
    ("lib.rs", include_str!("../lib.rs")),
    ("activation.rs", include_str!("../activation.rs")),
    ("jcs.rs", include_str!("../jcs.rs")),
    ("payload.rs", include_str!("../payload.rs")),
    ("policy.rs", include_str!("../policy.rs")),
    ("authority.rs", include_str!("../authority.rs")),
    ("text.rs", include_str!("../text.rs")),
    ("verdict.rs", include_str!("../verdict.rs")),
    ("verify.rs", include_str!("../verify.rs")),
    ("Cargo.toml", include_str!("../../Cargo.toml")),
];

const FORBIDDEN: [&str; 14] = [
    "std::fs",
    "std::net",
    "std::io",
    "std::env",
    "std::process",
    "std::thread",
    "std::time",
    "SystemTime",
    "Instant",
    "File::",
    "TcpStream",
    "reqwest",
    "tokio",
    "codeterm-",
];

#[test]
fn library_sources_perform_no_io_and_read_no_clock() {
    for (name, source) in SOURCES {
        let library = if name == "Cargo.toml" {
            source
                .split("[dev-dependencies]")
                .next()
                .unwrap_or(source)
                .replace("name = \"codeterm-entitlements\"", "")
        } else {
            source.to_string()
        };
        for token in FORBIDDEN {
            assert!(!library.contains(token), "{name} contains {token}");
        }
    }
}
