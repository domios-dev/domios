import { useCallback, useEffect, useRef, useState, type CSSProperties } from "react";
import { Check } from "lucide-react";
import { invoke } from "../../lib/bridge";
import { useConfig } from "../../hooks/useConfig";
import { webRequestContext, assertWebRequestContext } from "../../lib/webApi";
import { createPortal } from "react-dom";

type Presence = "online" | "offline";

interface MeshSection { enabled: boolean; [k: string]: unknown }
interface AppConfigShape { mesh: MeshSection; [k: string]: unknown }

async function setEnabled(next: boolean, generation: number): Promise<boolean> {
  try {
    assertWebRequestContext(generation);
    const cfg = await invoke<AppConfigShape>("get_config");
    assertWebRequestContext(generation);
    const updated = { ...cfg, mesh: { ...cfg.mesh, enabled: next } };
    await invoke("update_config", { config: updated });
    assertWebRequestContext(generation);
    return true;
  } catch {
    return false;
  }
}

export function MeshPresenceTray() {
  const { config, ready } = useConfig();
  const generation = webRequestContext();
  const [presence, setPresence] = useState<{ generation: number; value: Presence } | null>(null);
  const currentPresence = presence?.generation === generation ? presence.value : null;
  const state = ready ? currentPresence ?? (config.mesh.enabled ? "online" : "offline") : null;
  const [open, setOpen] = useState(false);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);

  useEffect(() => setPresence(null), [config]);

  const choose = useCallback(async (next: Presence) => {
    setOpen(false);
    if (!ready || state === next) return;
    const ok = await setEnabled(next === "online", generation);
    if (ok) setPresence({ generation, value: next });
  }, [state, ready, generation]);

  useEffect(() => {
    if (!open) return;
    const click = (e: MouseEvent) => {
      if (
        menuRef.current && !menuRef.current.contains(e.target as Node) &&
        triggerRef.current && !triggerRef.current.contains(e.target as Node)
      ) setOpen(false);
    };
    const key = (e: KeyboardEvent) => { if (e.key === "Escape") setOpen(false); };
    document.addEventListener("mousedown", click, true);
    document.addEventListener("keydown", key);
    return () => {
      document.removeEventListener("mousedown", click, true);
      document.removeEventListener("keydown", key);
    };
  }, [open]);

  const pos = (() => {
    if (!open || !triggerRef.current) return null;
    const rect = triggerRef.current.getBoundingClientRect();
    return { top: rect.bottom + 4, right: window.innerWidth - rect.right };
  })();

  const dotStyle =
    state === "online"
      ? { background: "var(--ct-green)", boxShadow: "0 0 6px color-mix(in srgb, var(--ct-green) 55%, transparent)" }
      : state === "offline"
        ? { background: "transparent", border: "1.5px solid var(--ct-muted, #888)" }
        : { background: "var(--ct-muted, #888)", opacity: 0.4 };

  const title = state === "online"
    ? "Mesh: online — peers see this machine"
    : state === "offline"
      ? "Mesh: offline — invisible to peers, Tailscale untouched"
      : "Mesh status…";

  return (
    <>
      <button
        ref={triggerRef}
        onClick={() => setOpen((v) => !v)}
        title={title}
        className="flex items-center justify-center w-7 h-7 rounded-md transition-colors hover:bg-[var(--ct-hover)]"
      >
        <span
          className="inline-block w-2.5 h-2.5 rounded-full transition-all"
          style={dotStyle}
        />
      </button>

      {open && pos && createPortal(
        <div
          ref={menuRef}
          className="ct-portal-menu ct-menu fixed w-[220px]"
          style={{ top: pos.top, right: pos.right, "--ct-menu-origin": "top right" } as CSSProperties}
        >
          <div className="ct-menu-caption">Mesh presence</div>
          <PresenceItem
            label="Online"
            sub="Visible to peers"
            active={state === "online"}
            dotStyle={{ background: "var(--ct-green)", boxShadow: "0 0 6px color-mix(in srgb, var(--ct-green) 55%, transparent)" }}
            onClick={() => choose("online")}
          />
          <PresenceItem
            label="Offline"
            sub="Invisible — peers won't see this machine"
            active={state === "offline"}
            dotStyle={{ background: "transparent", border: "1.5px solid var(--ct-muted, #888)" }}
            onClick={() => choose("offline")}
          />
        </div>,
        document.body
      )}
    </>
  );
}

function PresenceItem({
  label, sub, active, dotStyle, onClick,
}: {
  label: string;
  sub: string;
  active: boolean;
  dotStyle: CSSProperties;
  onClick: () => void;
}) {
  return (
    <button onClick={onClick} className="ct-menu-row">
      <span className="ct-menu-row__icon">
        <span className="inline-block w-2.5 h-2.5 rounded-full" style={dotStyle} />
      </span>
      <span className="min-w-0 flex-1">
        <span className={`block truncate ${active ? "font-medium" : ""}`}>{label}</span>
        <span className="ct-menu-row__description">{sub}</span>
      </span>
      {active && <Check aria-hidden size={14} className="ct-menu-row__check shrink-0" />}
    </button>
  );
}
