/// Static branded pages for moments when the React bundle cannot load, so they cannot reuse its components.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BrandFallback {
    /// The native client lost its interface and retries on its own.
    Reconnecting,
    /// The server is running but has no frontend files to serve.
    InterfaceMissing,
}

/// The native window is frameless; these links are handled by the client as `domios-shell://<action>`.
const NATIVE_CHROME: &str = r#"<header><div class="drag" onmousedown="if(event.button===0)location.href='domios-shell://drag'"></div><a href="domios-shell://minimize" aria-label="Minimize">&#8212;</a><a href="domios-shell://maximize" aria-label="Maximize">&#9633;</a><a class="close" href="domios-shell://close" aria-label="Close">&#10005;</a></header>"#;

const RETRY_ACTION: &str = r#"<a class="retry" href="domios-shell://retry">Retry now</a>"#;

/// Faint dot grid with soft waves spreading from the centre, matching the in-app brand matrix.
const DOT_WAVES: &str = r#"<script>
(() => {
  const canvas = document.querySelector("canvas");
  const context = canvas.getContext("2d");
  const reduced = matchMedia("(prefers-reduced-motion: reduce)").matches;
  const spacing = 18, rest = 0.018, peak = 0.16, period = 2600, band = 26;
  function frame(now) {
    const ratio = devicePixelRatio || 1;
    const width = canvas.clientWidth, height = canvas.clientHeight;
    if (canvas.width !== width * ratio || canvas.height !== height * ratio) {
      canvas.width = width * ratio; canvas.height = height * ratio;
    }
    context.setTransform(ratio, 0, 0, ratio, 0, 0);
    context.clearRect(0, 0, width, height);
    context.fillStyle = getComputedStyle(canvas).color;
    const cx = width / 2, cy = height / 2, extent = Math.hypot(cx, cy);
    const radius = ((now % period) / period) * extent;
    for (let x = spacing / 2; x < width; x += spacing) {
      for (let y = spacing / 2; y < height; y += spacing) {
        const distance = Math.hypot(x - cx, y - cy);
        let alpha = rest * Math.max(0, 1 - distance / extent);
        if (!reduced) {
          const offset = (distance - radius) / band;
          alpha = Math.max(alpha, peak * Math.exp(-0.5 * offset * offset) * (1 - distance / extent));
        }
        if (alpha <= 0.002) continue;
        context.globalAlpha = alpha;
        context.beginPath();
        context.arc(x, y, 1.2, 0, Math.PI * 2);
        context.fill();
      }
    }
    if (!reduced) requestAnimationFrame(frame);
  }
  requestAnimationFrame(frame);
})();
</script>"#;

pub fn brand_fallback_page(kind: BrandFallback) -> String {
    let (title, heading, body, role, pulse, chrome, canvas, action, script) = match kind {
        BrandFallback::Reconnecting => (
            "Reconnecting",
            "Reconnecting…",
            "Your sessions keep running. Domios retries automatically.",
            "status",
            " data-pulse",
            NATIVE_CHROME,
            r#"<canvas aria-hidden="true"></canvas>"#,
            RETRY_ACTION,
            DOT_WAVES,
        ),
        BrandFallback::InterfaceMissing => (
            "Interface unavailable",
            "The interface could not load",
            "Domios is running, but its frontend files are unavailable. Restore or rebuild the frontend, then reload this page.",
            "alert",
            "",
            "",
            "",
            "",
            "",
        ),
    };
    include_str!("brand_fallback.html")
        .replace("{{TITLE}}", title)
        .replace("{{HEADING}}", heading)
        .replace("{{BODY}}", body)
        .replace("{{ROLE}}", role)
        .replace("{{PULSE}}", pulse)
        .replace("{{CHROME}}", chrome)
        .replace("{{CANVAS}}", canvas)
        .replace("{{ACTION}}", action)
        .replace("{{SCRIPT}}", script)
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
        }
        let reconnecting = brand_fallback_page(BrandFallback::Reconnecting);
        assert!(reconnecting.contains("domios-shell://retry"));
        assert!(reconnecting.contains("domios-shell://close"));
        let missing = brand_fallback_page(BrandFallback::InterfaceMissing);
        assert!(!missing.contains("<script"));
        assert!(!missing.contains("domios-shell://"));
    }
}
