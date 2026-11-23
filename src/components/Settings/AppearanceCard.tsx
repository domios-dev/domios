import { useEffect, useState } from "react";
import { Check, FlaskConical, Info, Palette } from "lucide-react";
import { deriveColor } from "../../lib/colors";
import { DEFAULT_APPEARANCE, type AppearanceProposal } from "../../lib/appearance";
import { APPEARANCE_CHANGED, OPEN_APPEARANCE, appearanceError, changeAppearance, listenAppearanceCommit, readAppearanceProposal, type AppearanceTarget, type OpenAppearance } from "../../lib/appearanceClient";
import { clearAppearancePress, indicateAppearancePress } from "../../lib/appearanceMotion";
import { useAccountMachineNames, useMeshMachineChoices, useMeshPeerName } from "../../lib/useMeshMachineIdentities";
import { cleanMachineName } from "../../lib/machineIdentity";
import { effectiveWallpaper, WALLPAPER_PATTERNS, wallpaperDefaultOpacity, wallpaperSummary } from "../../lib/chatWallpaper";
import { isLightColor } from "../../lib/themeCss";
import { Button } from "../shared/Button";
import { AppearancePreview } from "./AppearancePreview";
import "./appearance.css";

function changes(proposal: AppearanceProposal, wallpaperLabel: string | null): string[] {
  const before = proposal.previous.tuning ?? DEFAULT_APPEARANCE;
  const after = proposal.look.tuning ?? DEFAULT_APPEARANCE;
  const { chat_background: _wallpaper, ...theme } = proposal.look.theme;
  const { chat_background: _previousWallpaper, ...previousTheme } = proposal.previous.theme;
  const previousStrength = proposal.previous.wallpaper?.fill_strength ?? 1;
  const strength = proposal.look.wallpaper?.fill_strength ?? 1;
  return [
    JSON.stringify(theme) !== JSON.stringify(previousTheme) ? "Theme" : null,
    wallpaperLabel,
    after.roundness !== before.roundness ? `Roundness ${after.roundness}` : null,
    after.shadow_strength !== before.shadow_strength ? `Shadow ${after.shadow_strength}` : null,
    after.edges !== before.edges ? `Edges ${after.edges}` : null,
    strength !== previousStrength ? `Background ${Math.round(strength * 100)}%` : null,
    after.font_size !== before.font_size ? `Interface text ${after.font_size > 0 ? "+" : ""}${after.font_size}px` : null,
  ].filter((value): value is string => value !== null);
}

function tweakChanges({ previous, look }: AppearanceProposal): string[] {
  const before = previous.tuning ?? DEFAULT_APPEARANCE;
  const after = look.tuning ?? DEFAULT_APPEARANCE;
  const intensity = ({ theme, wallpaper }: AppearanceProposal["look"]) => {
    const { background, palette } = effectiveWallpaper(theme, wallpaper ?? undefined);
    const painted = Boolean(palette || background.color || background.colors?.length);
    return background.opacity ?? wallpaperDefaultOpacity(background.pattern, isLightColor(theme.background), painted);
  };
  const percent = (value: number) => `${Number((value * 100).toFixed(2))}%`;
  const pixels = (value: number) => `${value > 0 ? "+" : ""}${value}px`;
  const controls = [
    ["Wallpaper", percent(intensity(previous)), percent(intensity(look))],
    ["Background", percent(previous.wallpaper?.fill_strength ?? 1), percent(look.wallpaper?.fill_strength ?? 1)],
    ["Roundness", percent(before.roundness), percent(after.roundness)],
    ["Shadow", percent(before.shadow_strength), percent(after.shadow_strength)],
    ["Edges", before.edges, after.edges],
    ["Interface text", pixels(before.font_size), pixels(after.font_size)],
  ];
  return controls.flatMap(([label, from, to]) => from === to ? [] : [`${label} ${from} → ${to}`]);
}

function AppearanceCard({ id, machine }: { id: string; machine: string }) {
  const machines = useMeshMachineChoices();
  const accountNames = useAccountMachineNames();
  const matchingPeers = machines.peers?.filter(candidate => candidate.instance_name === machine);
  const peer = machines.peers?.find(candidate => candidate.instance_id === machine)?.instance_id ??
    (machine !== machines.local_instance_name && matchingPeers?.length === 1 ? matchingPeers[0].instance_id : undefined);
  const peerName = useMeshPeerName(peer);
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
  const name = (peer ? peerName : accountNames.local ?? machines.local_instance_name)
    ?? cleanMachineName(record.machine.name, record.machine.id) ?? "Machine";
  const theme = look.theme;
  const themeWallpaper = theme.chat_background;
  const hasThemeWallpaper = Boolean(themeWallpaper && (themeWallpaper.pattern !== "none" || themeWallpaper.gradient));
  const hidesThemeWallpaper = hasThemeWallpaper && look.wallpaper !== null;
  const exposesThemeWallpaper = hasThemeWallpaper && look.wallpaper === null && record.previous.wallpaper !== null;
  const appliedTime = record.applied_at === null ? "" : new Date(record.applied_at * 1000).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  const stateText = state === "proposed" ? (peer ? `For ${name}` : "Proposal") : state === "applied"
    ? `✓ Applied${peer ? ` to ${name}` : ""}${appliedTime ? ` · ${appliedTime}` : ""}`
    : record.tweak ? "↶ Undone" : `↶ Undone · back to ${record.previous.theme.name}`;
  const themeWallpaperName = themeWallpaper?.pattern === "none" ? undefined
    : WALLPAPER_PATTERNS.find(pattern => pattern.id === themeWallpaper?.pattern)?.label
      ?? (themeWallpaper?.pattern === "svg" ? "SVG" : themeWallpaper?.pattern);
  const wallpaper = effectiveWallpaper(theme, look.wallpaper ?? undefined);
  const previousWallpaper = effectiveWallpaper(record.previous.theme, record.previous.wallpaper ?? undefined);
  const overrideWallpaper = look.wallpaper ? wallpaperSummary(theme, look.wallpaper) : "";
  const wallpaperLabel = hidesThemeWallpaper ? `Theme wallpaper${themeWallpaperName ? `: ${themeWallpaperName}` : ""}`
    : wallpaper.background.pattern !== "none" || JSON.stringify(wallpaper) !== JSON.stringify(previousWallpaper)
      ? wallpaperSummary(theme, look.wallpaper ?? undefined).replaceAll(" · ", " ") : null;
  const applyLabel = exposesThemeWallpaper ? "Use theme wallpaper"
    : state === "undone" ? "Apply again" : peer ? `Apply to ${name}` : "Apply";
  const swatches = [
    ["Base", theme.background], ["Surface", deriveColor(theme.background, 10)], ["Ink", theme.foreground],
    ["Accent", theme.blue], ["Success", theme.green], ["Warning", theme.yellow], ["Error", theme.red],
  ];
  const ratios = ["Text", "Muted", "Accent", "Error"].flatMap(name =>
    record.contrast.filter(value => value.name === name).sort((a, b) => a.ratio - b.ratio).slice(0, 1));
  const run = async (action: "apply" | "undo" | "use-theme-wallpaper", button: HTMLButtonElement) => {
    if (busy) return;
    if (action !== "undo") indicateAppearancePress(button);
    setBusy(true);
    setError("");
    try { setRecord(await changeAppearance(target, id, action)); }
    catch (failure) { setError(appearanceError(failure)); }
    finally { setBusy(false); clearAppearancePress(button); }
  };
  const openLab = () => window.dispatchEvent(new CustomEvent<OpenAppearance>(OPEN_APPEARANCE, { detail: { ...target, proposal: id } }));
  return (
    <section role="group" aria-label={`Appearance ${state}: ${theme.name}`} className="ct-appearance-card" data-state={state} data-appearance-proposal={id}>
      {!record.tweak && <div className="ct-appearance-card__body">
        <div className="ct-appearance-card__detail">
          <div>
            <h3><Palette size={14} aria-hidden="true" />{theme.name}</h3>
            {record.rationale && <p>{record.rationale}</p>}
          </div>
          {exposesThemeWallpaper && <p>This look uses the theme wallpaper and clears the machine override.</p>}
          <div className="ct-appearance-card__swatches">{swatches.map(([label, value]) =>
            <span key={label} role="img" aria-label={`${label} ${value}`}><i style={{ background: value }} aria-hidden="true" />{label}</span>)}</div>
          {ratios.length > 0 && <div className="ct-appearance-card__ratios">
            {ratios.map(check => <span key={check.name}>{check.name} <b>{check.ratio.toFixed(1)}:1</b></span>)}
            {record.contrast.every(check => check.ratio >= check.floor) && <span className="ct-appearance-card__readable"><Check size={12} aria-hidden="true" />all readable</span>}
          </div>}
          <div className="ct-appearance-card__changes">
            {changes(record, wallpaperLabel).map(change => <span key={change}>{change}</span>)}
          </div>
        </div>
        <div className="ct-appearance-card__preview"><AppearancePreview compact look={look} peer={target.peer} /></div>
      </div>}
      {!record.tweak && hidesThemeWallpaper && <div className="ct-appearance-card__wallpaper">
        <Info size={13} aria-hidden="true" />
        <p>This look includes {themeWallpaperName ? `${themeWallpaperName} wallpaper` : "a theme wallpaper"}, but the machine’s wallpaper ({overrideWallpaper}) stays on top.{" "}
          <button type="button" className="ct-appearance-card__wallpaper-link" disabled={busy || (state === "applied" && !record.can_undo)}
            onClick={event => void run("use-theme-wallpaper", event.currentTarget)}>Use theme wallpaper</button>
        </p>
      </div>}
      {error && <p className="ct-appearance-error" role="alert">{error}</p>}
      <footer>
        {record.tweak && <div className="ct-appearance-card__changes">
          {tweakChanges(record).map(change => <span key={change}>{change}</span>)}
        </div>}
        {state === "applied" ? <Button variant="outline" disabled={busy || !record.can_undo} onClick={event => void run("undo", event.currentTarget)}>Undo</Button>
          : !record.tweak && <Button variant={state === "undone" ? "outline" : "primary"} disabled={busy} onClick={event => void run("apply", event.currentTarget)}>{applyLabel}</Button>}
        <Button variant="ghost" onClick={openLab}><FlaskConical size={13} aria-hidden="true" /> Open in Lab</Button>
        <small className="ct-appearance-card__state" role="status">{stateText}</small>
      </footer>
    </section>
  );
}

export function renderAppearanceCard(id: string, machine: string) {
  return <AppearanceCard id={id} machine={machine} />;
}
