/// Static branded pages for moments when the React bundle cannot load, so they cannot reuse its components.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BrandFallback {
    /// The native client is starting and its interface is not painted yet.
    Starting,
    /// The native client lost its interface.
    Reconnecting,
    /// Startup failed; the URL fragment carries the reason.
    StartFailed,
    /// The server is running but has no frontend files to serve.
    InterfaceMissing,
}

/// The native window is frameless; these links are handled by the client as `domios-shell://<action>`.
const NATIVE_CHROME: &str = r#"<header><div class="drag" onmousedown="if(event.button===0)location.href='domios-shell://drag'"></div><a href="domios-shell://minimize" aria-label="Minimize">&#8212;</a><a href="domios-shell://maximize" aria-label="Maximize">&#9633;</a><a class="close" href="domios-shell://close" aria-label="Close">&#10005;</a></header>"#;

const RETRY_ACTION: &str =
    r#"<a class="retry" href="domios-shell://retry" aria-label="Retry now">Retry now</a>"#;

/// A stated refusal repeats on every attempt, so this page retries only when asked.
const START_FAILED_ACTION: &str = r#"<p class="detail" hidden></p><a class="retry" href="domios-shell://retry">Retry</a><a class="retry" href="domios-shell://quit">Quit</a>"#;

/// The reason travels in the fragment so the document stays fixed for navigation checks; textContent keeps it inert.
const START_FAILED_SCRIPT: &str = r#"<script>
(() => {
  const detail = document.querySelector(".detail");
  const show = () => {
    let text = "";
    try { text = decodeURIComponent(location.hash.slice(1)); } catch {}
    detail.textContent = text;
    detail.hidden = !text;
  };
  addEventListener("hashchange", show);
  show();
})();
</script>"#;

/// Faint dot grid; each wave leaves the mark at its brightest beat (CSS and canvas share one 2.6 s period).
/// The beat curve and wave timing must match `src/components/brand/useBrandMotion.ts` (`beatLevel`, `BEAT_PEAK`);
/// grid step, dot radius and band width must match `src/components/brand/dotMatrixModel.ts`.
const DOT_WAVES: &str = r#"<script>
(() => {
  const canvas = document.querySelector("canvas");
  const context = canvas.getContext("2d");
  const reduced = matchMedia("(prefers-reduced-motion: reduce)").matches;
  const spacing = 18, rest = 0.018, peak = 0.16, period = 2600, band = 26, beatPeak = 0.08;
  function frame(now) {
    const ratio = devicePixelRatio || 1;
    const width = canvas.clientWidth, height = canvas.clientHeight;
    if (canvas.width !== width * ratio || canvas.height !== height * ratio) {
      canvas.width = width * ratio; canvas.height = height * ratio;
    }
    context.setTransform(ratio, 0, 0, ratio, 0, 0);
    context.clearRect(0, 0, width, height);
    context.fillStyle = getComputedStyle(canvas).color;
    const mark = document.querySelector("main svg").getBoundingClientRect();
    const box = canvas.getBoundingClientRect();
    const cx = mark.left + mark.width / 2 - box.left, cy = mark.top + mark.height / 2 - box.top;
    const extent = Math.max(Math.hypot(cx, cy), Math.hypot(width - cx, height - cy));
    const start = mark.width / 2;
    const sinceBeat = ((now - beatPeak * period) % period + period) % period;
    const radius = start + (sinceBeat / period) * (extent - start);
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
            "Your sessions keep running.",
            "status",
            " data-pulse",
            NATIVE_CHROME,
            r#"<canvas aria-hidden="true"></canvas>"#,
            RETRY_ACTION,
            DOT_WAVES,
        ),
        BrandFallback::Starting => (
            "Starting",
            "Starting Domios…",
            "Restoring your sessions.",
            "status",
            " data-pulse",
            NATIVE_CHROME,
            r#"<canvas aria-hidden="true"></canvas>"#,
            "",
            DOT_WAVES,
        ),
        BrandFallback::StartFailed => (
            "Could not start",
            "Domios could not start",
            "Startup could not finish. Resolve the reason below, then retry.",
            "alert",
            "",
            NATIVE_CHROME,
            "",
            START_FAILED_ACTION,
            START_FAILED_SCRIPT,
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
mod tests;
