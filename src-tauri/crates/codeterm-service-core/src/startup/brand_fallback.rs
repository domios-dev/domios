/// Static branded pages for moments when the React bundle cannot load, so they cannot reuse its components.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BrandFallback {
    /// The native client lost its interface and retries on its own.
    Reconnecting,
    /// The server is running but has no frontend files to serve.
    InterfaceMissing,
}

pub fn brand_fallback_page(kind: BrandFallback) -> String {
    let (title, heading, body, role, pulse) = match kind {
        BrandFallback::Reconnecting => (
            "Reconnecting",
            "Reconnecting…",
            "Your sessions keep running. Domios retries automatically.",
            "status",
            " data-pulse",
        ),
        BrandFallback::InterfaceMissing => (
            "Interface unavailable",
            "The interface could not load",
            "Domios is running, but its frontend files are unavailable. Restore or rebuild the frontend, then reload this page.",
            "alert",
            "",
        ),
    };
    include_str!("brand_fallback.html")
        .replace("{{TITLE}}", title)
        .replace("{{HEADING}}", heading)
        .replace("{{BODY}}", body)
        .replace("{{ROLE}}", role)
        .replace("{{PULSE}}", pulse)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_pages_share_the_mark_and_fill_every_slot() {
        for kind in [BrandFallback::Reconnecting, BrandFallback::InterfaceMissing] {
            let page = brand_fallback_page(kind);
            assert!(page.contains("M8 8H32C46 8 56 18 56 32S46 56 32 56H24L40 40V24H24V40L8 56Z"));
            assert!(!page.contains("{{"));
            assert!(!page.contains("<script"));
        }
        assert!(brand_fallback_page(BrandFallback::Reconnecting).contains("data-pulse"));
        assert!(!brand_fallback_page(BrandFallback::InterfaceMissing).contains(" data-pulse"));
    }
}
