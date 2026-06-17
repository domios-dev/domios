// Phase-1 pipeline proof: a real React UI, bundled by scripts/build-plugin-ui.mjs
// into a single nonce'd index.html, talking to the host via window.ct.
import { useEffect, useState } from "react";
import { createRoot } from "react-dom/client";

declare global {
  interface Window {
    ct?: { invoke(method: string, args?: unknown): Promise<unknown> };
  }
}

function App() {
  const [out, setOut] = useState("…");
  useEffect(() => {
    window.ct
      ?.invoke("echo", { hello: "react" })
      .then((r) => setOut("ready: " + JSON.stringify(r)))
      .catch((e) => setOut("ERR " + e));
  }, []);
  return (
    <div style={{ fontFamily: "system-ui, sans-serif", padding: 20, color: "var(--ct-fg, #eee)" }}>
      <h3>React plugin view ✓ (bundled)</h3>
      <button
        style={{
          font: "inherit",
          padding: "6px 12px",
          borderRadius: 6,
          border: "1px solid var(--ct-accent, #5b8cff)",
          background: "transparent",
          color: "var(--ct-accent, #5b8cff)",
          cursor: "pointer",
        }}
        onClick={() =>
          window.ct?.invoke("platform").then((r) => setOut("platform → " + JSON.stringify(r)))
        }
      >
        Call host.platform() via bridge
      </button>
      <pre style={{ marginTop: 14, color: "var(--ct-muted, #9aa)" }}>{out}</pre>
    </div>
  );
}

const el = document.getElementById("ct-root");
if (el) createRoot(el).render(<App />);
