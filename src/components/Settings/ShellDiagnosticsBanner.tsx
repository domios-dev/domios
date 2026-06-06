import { useEffect, useState } from "react";
import { invoke } from "../../lib/bridge";
import { Sliders } from "lucide-react";

interface ShellDiagnostics {
  platform: string;
  pwsh_installed: boolean;
  pwsh_path?: string | null;
  powershell_path?: string | null;
  current_shell: string;
}

export function ShellDiagnosticsBanner({ onDismissSettings }: { onDismissSettings: () => void }) {
  const [diag, setDiag] = useState<ShellDiagnostics | null>(null);

  useEffect(() => {
    invoke<ShellDiagnostics>("get_shell_diagnostics").then(setDiag).catch(() => {});
  }, []);

  if (!diag || diag.platform !== "windows" || diag.pwsh_installed) return null;

  const handleInstall = async () => {
    try {
      await invoke("suggest_install_pwsh");
      onDismissSettings();
    } catch (e) {
      console.warn("suggest_install_pwsh failed:", e);
    }
  };

  const handleRestart = async () => {
    try {
      await invoke("restart_app");
    } catch (e) {
      console.warn("restart_app failed:", e);
    }
  };

  return (
    <div
      className="rounded-lg border p-3 flex items-start gap-3"
      style={{
        background: "color-mix(in srgb, var(--ct-yellow) 8%, var(--ct-bg))",
        borderColor: "color-mix(in srgb, var(--ct-yellow) 35%, transparent)",
      }}
    >
      <div className="w-8 h-8 rounded-md flex items-center justify-center shrink-0"
        style={{ background: "color-mix(in srgb, var(--ct-yellow) 20%, transparent)", color: "var(--ct-yellow)" }}>
        <Sliders size={16} />
      </div>
      <div className="flex-1 min-w-0">
        <div className="text-[13px] font-medium text-[var(--ct-fg)]">Faster shell available</div>
        <div className="text-[11px] text-[var(--ct-muted-fg)] mt-0.5">
          You're on Windows PowerShell 5.1 which cold-starts slowly and stalls on Defender AMSI.
          PowerShell 7 (pwsh) starts 5–10× faster. CodeTerm will auto-detect it after install.
        </div>
        <div className="flex gap-1.5 mt-2">
          <button
            onClick={handleInstall}
            className="text-[11px] px-2.5 py-1 rounded border bg-[var(--ct-accent)] text-[var(--ct-bg)] border-[var(--ct-accent)] hover:opacity-90"
          >
            Install via winget
          </button>
          <button
            onClick={handleRestart}
            className="text-[11px] px-2.5 py-1 rounded border bg-transparent text-[var(--ct-muted-fg)] border-[var(--ct-border)] hover:text-[var(--ct-fg)]"
          >
            Restart CodeTerm
          </button>
        </div>
      </div>
    </div>
  );
}
