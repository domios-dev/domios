import { useEffect, useState } from "react";
import { Palette } from "lucide-react";
import { deriveColor } from "../../lib/colors";
import { DEFAULT_APPEARANCE, type AppearanceProposal } from "../../lib/appearance";
import { APPEARANCE_CHANGED, OPEN_APPEARANCE, appearanceError, changeAppearance, listenAppearanceCommit, readAppearanceProposal, type AppearanceTarget, type OpenAppearance } from "../../lib/appearanceClient";
import { bindAppearancePress, clearAppearancePress, indicateAppearancePress } from "../../lib/appearanceMotion";
import { useMeshMachineChoices } from "../../lib/useMeshMachineIdentities";
import { wallpaperSummary } from "../../lib/chatWallpaper";
import { relayTarget } from "../../lib/webApi";
import { AppearancePreview } from "./AppearancePreview";
import "./appearance.css";

function changes(proposal: AppearanceProposal): string[] {
  const before = proposal.previous.tuning ?? DEFAULT_APPEARANCE;
  const after = proposal.look.tuning ?? DEFAULT_APPEARANCE;
  const { chat_background: wallpaper, ...theme } = proposal.look.theme;
  const { chat_background: previousWallpaper, ...previousTheme } = proposal.previous.theme;
  const previousStrength = proposal.previous.wallpaper?.fill_strength ?? 1;
  const strength = proposal.look.wallpaper?.fill_strength ?? 1;
  return [
    JSON.stringify(theme) !== JSON.stringify(previousTheme) ? "Theme" : null,
    JSON.stringify([wallpaper, proposal.look.wallpaper]) !== JSON.stringify([previousWallpaper, proposal.previous.wallpaper]) ? "Wallpaper" : null,
    after.roundness !== before.roundness ? `Roundness ${after.roundness}` : null,
    after.shadow_strength !== before.shadow_strength ? `Shadow ${after.shadow_strength}` : null,
    after.edges !== before.edges ? `Edges ${after.edges}` : null,
    strength !== previousStrength ? `Background ${Math.round(strength * 100)}%` : null,
    after.font_size !== before.font_size ? `Font ${after.font_size > 0 ? "+" : ""}${after.font_size}px` : null,
  ].filter((value): value is string => value !== null);
}

function AppearanceCard({ id, machine }: { id: string; machine: string }) {
  const machines = useMeshMachineChoices();
  const matchingPeers = machines.peers?.filter(candidate => candidate.instance_name === machine);
  const peer = machines.peers?.find(candidate => candidate.instance_id === machine)?.instance_id ??
    (machine !== machines.local_instance_name && matchingPeers?.length === 1 ? matchingPeers[0].instance_id : undefined);
  const target: AppearanceTarget = { machine, peer };
  const [record, setRecord] = useState<AppearanceProposal>();
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const recordMachine = record?.machine.id ?? machine;
  useEffect(() => {
    let active = true;
    let release: (() => void) | undefined;
    const refresh = () => readAppearanceProposal({ machine, peer }, id).then(
      value => { if (active) { setRecord(value); setError(""); } },
      () => { if (active) setError("Appearance proposal is unavailable."); },
    );
    void refresh();
    void listenAppearanceCommit(commit => {
      if (commit.machine === recordMachine) void refresh();
    }, peer).then(fn => { if (active) release = fn; else fn(); }).catch(() => {});
    const onChange = (event: Event) => {
      const next = (event as CustomEvent<{ receipt: AppearanceProposal }>).detail?.receipt;
      if (next?.machine.id !== recordMachine) return;
      if (next?.id === id) setRecord(next);
      else if (next) setRecord(current => current && { ...current, can_undo: false });
    };
    window.addEventListener(APPEARANCE_CHANGED, onChange);
    return () => { active = false; release?.(); window.removeEventListener(APPEARANCE_CHANGED, onChange); };
  }, [id, machine, peer, recordMachine]);

  if (!record) return <p role="status">{error || "Reading appearance proposal…"}</p>;
  const { state, look } = record;
  const name = record.machine.name;
  const theme = look.theme;
  const themeWallpaper = theme.chat_background;
  const hasThemeWallpaper = Boolean(themeWallpaper && (themeWallpaper.pattern !== "none" || themeWallpaper.gradient));
  const hidesThemeWallpaper = hasThemeWallpaper && look.wallpaper !== null;
  const exposesThemeWallpaper = hasThemeWallpaper && look.wallpaper === null && record.previous.wallpaper !== null;
  const stateText = state === "proposed" ? `Proposed · for ${name}` : state === "applied"
    ? `✓ Applied to ${name}` : `↶ Undone · back to ${record.previous.theme.name}`;
  const swatches = [
    ["Base", theme.background], ["Surface", deriveColor(theme.background, 10)], ["Ink", theme.foreground],
    ["Accent", theme.blue], ["Success", theme.green], ["Warning", theme.yellow], ["Error", theme.red],
  ];
  const ratios = ["Text", "Muted", "Accent", "Error"].flatMap(name =>
    record.contrast.filter(value => value.name === name).sort((a, b) => a.ratio - b.ratio).slice(0, 1));
  const run = async (action: "apply" | "undo" | "use-theme-wallpaper", button: HTMLButtonElement) => {
    if (busy) return;
    if (action !== "undo") {
      indicateAppearancePress(button);
      bindAppearancePress(button, id, action === "use-theme-wallpaper" ? { ...look, wallpaper: null } : look, peer ?? relayTarget()?.id);
    }
    setBusy(true);
    setError("");
    try { setRecord(await changeAppearance(target, id, action)); }
    catch (failure) { clearAppearancePress(button); setError(appearanceError(failure)); }
    finally { setBusy(false); }
  };
  const openLab = () => window.dispatchEvent(new CustomEvent<OpenAppearance>(OPEN_APPEARANCE, { detail: { ...target, proposal: id } }));
  return (
    <section role="group" aria-label={`Appearance ${state}: ${theme.name}`} className="ct-appearance-card" data-state={state} data-appearance-proposal={id}>
      <header><strong><Palette size={14} aria-hidden="true" /> APPEARANCE</strong><span className="ct-appearance-card__state" role="status">{stateText}</span></header>
      <div className="ct-appearance-card__body">
        <div>
          <h3>{theme.name}</h3><p>{record.rationale}</p>
          {hidesThemeWallpaper && <p>The machine wallpaper hides this theme’s wallpaper.</p>}
          {exposesThemeWallpaper && <p>This look uses the theme wallpaper and clears the machine override.</p>}
          <div className="ct-appearance-card__swatches">{swatches.map(([label, value]) =>
            <span key={label}><i style={{ background: value }} aria-hidden="true" />{label}</span>)}</div>
          <div className="ct-appearance-card__ratios">{ratios.map(check =>
            <span key={check.name}>{check.name} <b>{check.ratio.toFixed(1)}:1 ✓</b></span>)}</div>
          <div className="ct-appearance-card__changes">{changes(record).map(change => <span key={change}>{change}</span>)}</div>
        </div>
        <figure className="ct-appearance-card__preview"><AppearancePreview compact look={look} peer={target.peer} /><figcaption>{wallpaperSummary(look.theme, look.wallpaper ?? undefined)}</figcaption></figure>
      </div>
      {error && <p className="ct-appearance-error" role="alert">{error}</p>}
      <footer>
        {state === "applied" ? <button type="button" disabled={busy || !record.can_undo} onClick={event => void run("undo", event.currentTarget)}>Undo</button>
          : <button type="button" className="ct-appearance-primary" disabled={busy} onClick={event => void run("apply", event.currentTarget)}>{exposesThemeWallpaper ? "Use theme wallpaper" : state === "undone" ? "Apply again" : `Apply to ${name}`}</button>}
        {hidesThemeWallpaper && <button type="button" disabled={busy || (state === "applied" && !record.can_undo)} onClick={event => void run("use-theme-wallpaper", event.currentTarget)}>Use theme wallpaper</button>}
        <button type="button" onClick={openLab}>Open in Lab</button>
        {state === "applied" && record.applied_at !== null && <small>Applied {new Date(record.applied_at * 1000).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}</small>}
      </footer>
    </section>
  );
}

export function renderAppearanceCard(id: string, machine: string) {
  return <AppearanceCard id={id} machine={machine} />;
}
