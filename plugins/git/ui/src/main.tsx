// Git panel — the plugin's own UI, rendered in the host's sandboxed iframe.
// Talks to the plugin's viewCall ops via window.ct (through gitApi). The host
// stamps the pane cwd into calls that omit it, so we resolve the active repo
// once (gitRepos with no cwd → host fills pane cwd) and hand the panel a
// concrete path it can then drive (and switch between, for multi-repo dirs).
import { useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import { GitPanel } from "./GitPanel";
import { fetchRepos } from "./gitApi";
import "./app.css";

function Root() {
  const [cwd, setCwd] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let active = true;
    fetchRepos(null, "")
      .then((repos) => {
        if (!active) return;
        setCwd(repos.length > 0 ? repos[0].path : "");
      })
      .catch((e) => {
        if (active) setError(String(e instanceof Error ? e.message : e));
      });
    return () => {
      active = false;
    };
  }, []);

  if (error) {
    return (
      <div style={{ padding: 18, color: "var(--ct-err, #e57373)", fontSize: 13, fontFamily: "system-ui, sans-serif" }}>
        {error}
      </div>
    );
  }
  if (cwd === null) {
    return (
      <div style={{ padding: 18, color: "var(--ct-muted, #9aa)", fontSize: 13, fontFamily: "system-ui, sans-serif" }}>
        Loading…
      </div>
    );
  }
  return <GitPanel api={null} cwd={cwd} onClose={() => window.ct?.close?.()} />;
}

const el = document.getElementById("ct-root");
if (el) createRoot(el).render(<Root />);
