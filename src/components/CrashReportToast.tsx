import { useEffect, useState } from "react";
import { invoke } from "../lib/bridge";
import { listen } from "../lib/bridge";
import { X, AlertTriangle, FolderOpen } from "lucide-react";

interface CrashPayload {
  dir: string;
  files: string[];
}

export function CrashReportToast() {
  const [payload, setPayload] = useState<CrashPayload | null>(null);

  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | null = null;
    (async () => {
      try {
        const u = await listen<CrashPayload>("crash_report_available", (e) => {
          if (!cancelled) setPayload(e.payload);
        });
        if (cancelled) { u(); return; }
        unlisten = u;
      } catch {
        /* event subsystem not ready — frontend reloaded mid-startup */
      }
    })();
    return () => { cancelled = true; if (unlisten) unlisten(); };
  }, []);

  if (!payload) return null;

  const dismiss = () => setPayload(null);
  const openFolder = () => {
    invoke("open_folder", { path: payload.dir }).catch(() => {});
  };

  const count = payload.files.length;
  const lead = count === 1 ? "Last session crashed" : `Last sessions crashed (${count} reports)`;

  return (
    <div className="fixed top-3 right-3 z-50 max-w-sm pointer-events-auto">
      <div
        className="rounded-lg px-3 py-2 border shadow-lg flex items-start gap-2"
        style={{
          background: "color-mix(in srgb, var(--ct-red) 12%, var(--ct-surface))",
          borderColor: "color-mix(in srgb, var(--ct-red) 35%, transparent)",
        }}
      >
        <AlertTriangle size={14} style={{ color: "var(--ct-red)" }} className="shrink-0 mt-0.5" />
        <div className="flex-1 min-w-0">
          <div className="text-[12px] font-semibold" style={{ color: "var(--ct-fg)" }}>
            {lead}
          </div>
          <div className="text-[10px] mt-0.5 truncate" style={{ color: "var(--ct-muted-fg)" }}>
            {payload.files[0]}
            {count > 1 ? ` + ${count - 1} more` : ""}
          </div>
          <button
            onClick={openFolder}
            className="mt-1.5 inline-flex items-center gap-1 text-[10px] px-2 py-0.5 rounded border"
            style={{
              color: "var(--ct-fg)",
              borderColor: "var(--ct-border)",
              background: "var(--ct-surface)",
            }}
          >
            <FolderOpen size={10} />
            Open folder
          </button>
        </div>
        <button
          onClick={dismiss}
          className="shrink-0 text-[var(--ct-muted)] hover:text-[var(--ct-fg)] transition-colors"
          aria-label="Dismiss"
        >
          <X size={12} strokeWidth={1.5} />
        </button>
      </div>
    </div>
  );
}
