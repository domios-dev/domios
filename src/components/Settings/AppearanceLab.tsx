import { useContext, useDeferredValue, useEffect, useId, useState, type FormEvent } from "react";
import { Monitor, Palette, Sparkles, Undo2 } from "lucide-react";
import { DEFAULT_APPEARANCE, PALETTE_CONTROLS, TERMINAL_COLOURS, applyContrastFix, paletteContrast, type AppearanceLook, type AppearanceProposal, type AppearanceTuning, type ContrastCheck, type CurrentAppearance } from "../../lib/appearance";
import { APPEARANCE_CHANGED, appearanceError, changeAppearance, checkAppearanceContrast, proposeAppearance, readAppearance, readAppearanceProposal, type AppearanceTarget, type OpenAppearance } from "../../lib/appearanceClient";
import { bindAppearancePress, clearAppearancePress, indicateAppearancePress } from "../../lib/appearanceMotion";
import { focusTab, generalAgentTab } from "../../lib/generalAgent";
import { setClientTabUiMode } from "../../lib/clientTabUiMode";
import { useMuxSelector } from "../../store/multiplexer/hooks";
import type { ExtendedState } from "../../store/multiplexer/tree";
import { sendTrackedChatMessage } from "../Terminal/chatSendStatus";
import { addChatSend } from "../Terminal/chatSendStore";
import { uid } from "../../lib/uid";
import { useAccountMachineNames, useMeshMachineChoices, useMeshPeerName } from "../../lib/useMeshMachineIdentities";
import { cleanMachineName } from "../../lib/machineIdentity";
import type { AppConfig, ThemeConfig } from "../../lib/types";
import { AppearancePreview } from "./AppearancePreview";
import { ChatWallpaperPicker } from "./ChatWallpaperPicker";
import { MachineAppearanceContext } from "../../lib/useMachineAppearance";
import { MultiplexerActionsContext, usePairedWorkspaceInventory } from "../../store/multiplexer/context";
import { relayTarget } from "../../lib/webApi";
import { SettingsCard } from "./shared";
import "./appearance.css";

const mainTab = (state: ExtendedState) => generalAgentTab(state)?.tabId;
const mainWorkspace = (state: ExtendedState) => generalAgentTab(state)?.workspaceId;
const PREFERRED_PRESET = "GitHub Dark";
const LOCAL: AppearanceTarget = { machine: "local" };
type Compare = "before" | "after" | "side-by-side";

interface Props {
  config: AppConfig;
  open?: OpenAppearance;
  showWallpaperOwn?: boolean;
  onNavigate(): void;
}

export function AppearanceLab({ config, open, showWallpaperOwn, onNavigate }: Props) {
  const heading = useId();
  const machineChoices = useMeshMachineChoices();
  const accountNames = useAccountMachineNames();
  const machineAppearance = useContext(MachineAppearanceContext);
  const viewedPeer = machineAppearance?.peerId;
  const [target, setTarget] = useState<AppearanceTarget>(() => open ?? (viewedPeer ? { machine: viewedPeer, peer: viewedPeer } : LOCAL));
  const peerName = useMeshPeerName(target.peer);
  const [current, setCurrent] = useState<CurrentAppearance>();
  const [draft, setDraft] = useState<AppearanceLook>();
  const [proposal, setProposal] = useState<AppearanceProposal>();
  const [compare, setCompare] = useState<Compare>("after");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [ask, setAsk] = useState(false);
  const [prompt, setPrompt] = useState("");
  const [contrast, setContrast] = useState<{ look: AppearanceLook; checks: ContrastCheck[] }>();
  const tabId = useMuxSelector(mainTab);
  const workspaceId = useMuxSelector(mainWorkspace);
  const actions = useContext(MultiplexerActionsContext);
  const { generals } = usePairedWorkspaceInventory();
  const remote = target.peer && target.peer !== relayTarget()?.id;
  const remoteGeneral = remote
    ? generals.find(peer => peer.peerId === target.peer && peer.availability === "online") : undefined;
  const remoteMain = remoteGeneral?.sessions.find(session => session.tab.protected);
  const generalTab = remote ? remoteMain?.viewerId : tabId;
  const appearanceOnline = !target.peer || machineChoices.peers?.some(peer => peer.instance_id === target.peer && peer.online);
  const checking = useDeferredValue(draft);
  useEffect(() => {
    if (!checking || !appearanceOnline) return;
    const controller = new AbortController();
    void checkAppearanceContrast(target, checking, controller.signal).then(checks => {
      if (!controller.signal.aborted) setContrast({ look: checking, checks });
    }).catch(() => {});
    return () => controller.abort();
  }, [target, checking, appearanceOnline]);

  useEffect(() => {
    let active = true;
    if (!appearanceOnline) return;
    const receipt = open?.proposal && open.machine === target.machine && open.peer === target.peer
      ? readAppearanceProposal(target, open.proposal) : undefined;
    Promise.all([readAppearance(target), receipt]).then(([next, stored]) => {
      if (!active) return;
      setCurrent(next);
      setDraft(stored?.look ?? next.look);
      setProposal(stored ?? next.active ?? undefined);
    }, (failure: unknown) => {
      if (active) setError(appearanceError(failure));
    });
    return () => { active = false; };
  }, [target, open, appearanceOnline]);

  useEffect(() => {
    const onChange = (event: Event) => {
      const detail = (event as CustomEvent<{ receipt: AppearanceProposal }>).detail;
      if (detail?.receipt.machine.id !== current?.machine.id) return;
      setProposal(detail.receipt);
      const look = detail.receipt.state === "undone" ? detail.receipt.previous : detail.receipt.look;
      setCurrent(previous => previous && { ...previous, look });
      setDraft(look);
    };
    window.addEventListener(APPEARANCE_CHANGED, onChange);
    return () => window.removeEventListener(APPEARANCE_CHANGED, onChange);
  }, [current?.machine.id]);

  const changeMachine = (id: string) => {
    setCurrent(undefined);
    setDraft(undefined);
    setProposal(undefined);
    setError("");
    setTarget(id === "local" ? LOCAL : { machine: id, peer: id });
  };
  const currentName = cleanMachineName(current?.machine.name, current?.machine.id);
  const localName = accountNames.local ?? machineChoices.local_instance_name ?? (!target.peer ? currentName : undefined);
  const machine = target.peer ? peerName ?? currentName ?? "Machine" : localName ?? "this machine";
  const unavailable = Boolean(target.peer && machineChoices.peers?.find(peer => peer.instance_id === target.peer)?.online === false);
  const tuning = draft?.tuning ?? DEFAULT_APPEARANCE;
  const dirty = Boolean(draft && current && JSON.stringify(draft) !== JSON.stringify(current.look));
  const failures = contrast?.look === draft ? contrast?.checks.filter(check => check.ratio < check.floor) ?? [] : [];
  const editTuning = (patch: Partial<AppearanceTuning>) => setDraft(value => value && { ...value, tuning: { ...tuning, ...patch } });
  const editTheme = (key: keyof ThemeConfig, colour: string) => setDraft(value => value && { ...value, theme: { ...value.theme, [key]: colour } });

  const apply = async (button: HTMLButtonElement) => {
    if (!draft || !current || busy) return;
    indicateAppearancePress(button);
    setBusy(true);
    setError("");
    try {
      const next = proposal?.state !== "applied" && proposal && JSON.stringify(draft) === JSON.stringify(proposal.look)
        ? proposal : await proposeAppearance(target, draft, current.look);
      bindAppearancePress(button, next.id, next.look, target.peer ?? relayTarget()?.id);
      await changeAppearance(target, next.id, "apply");
    } catch (failure) {
      clearAppearancePress(button);
      setError(appearanceError(failure));
    } finally {
      setBusy(false);
    }
  };

  const undo = async () => {
    if (!proposal?.can_undo || busy) return;
    setBusy(true);
    setError("");
    try {
      await changeAppearance(target, proposal.id, "undo");
    } catch (failure) {
      setError(appearanceError(failure));
    } finally {
      setBusy(false);
    }
  };

  const askGeneral = (event: FormEvent) => {
    event.preventDefault();
    const text = prompt.trim();
    if (!generalTab || !current || !text || busy) return;
    const { svg: _themeSvg, ...themeWallpaper } = current.look.theme.chat_background ?? {};
    const { svg: _wallpaperSvg, ...background } = current.look.wallpaper?.background ?? {};
    const look = { ...current.look,
      theme: { ...current.look.theme, chat_background: themeWallpaper },
      wallpaper: current.look.wallpaper && { ...current.look.wallpaper, background },
    };
    const name = machine.replace(/\s+/g, " ");
    const request = `Appearance request for ${name}: ${text}\n\n<context>\nAppearance Lab · for ${name}\nInstallation: ${current.machine.id}\nCurrent look: ${JSON.stringify(look)}\nSee domios docs APPEARANCE for tweaks and proposals.\n</context>`;
    setClientTabUiMode(generalTab, "chat");
    const id = uid();
    addChatSend(generalTab, { id, correlationId: id, content: request, sentAtMs: Date.now() });
    const sent = sendTrackedChatMessage(generalTab, id, request);
    onNavigate();
    if (remoteGeneral && remoteMain && workspaceId && actions?.attachRemoteTab) {
      actions.attachRemoteTab(workspaceId, { peer_id: remoteGeneral.peerId, peer_name: remoteGeneral.name, tabs: [remoteMain.tab] }, remoteMain.tab, remoteMain.status);
    } else {
      focusTab(generalTab, workspaceId ?? null);
    }
    void sent.catch(() => {});
  };

  const pickPreset = (theme: ThemeConfig) => setDraft(value => value && {
    ...value, theme: { ...theme, chat_background: value.theme.chat_background },
  });
  const reset = () => {
    const preset = current?.presets.find(value => value.name === PREFERRED_PRESET);
    if (preset) setDraft({ theme: preset, tuning: DEFAULT_APPEARANCE, wallpaper: null });
  };

  return (
    <section className="ct-appearance-lab" aria-labelledby={heading}>
      <header className="ct-appearance-lab__head">
        <h2 id={heading} className="sr-only"><Palette size={19} aria-hidden="true" /> Appearance Lab</h2>
        <label className="ct-appearance-machine"><Monitor size={15} aria-hidden="true" />
          <select aria-label="Appearance machine" value={target.peer ?? "local"} onChange={event => changeMachine(event.target.value)} disabled={busy}>
            <option value="local">For {localName ?? "this machine"}</option>
            {machineChoices.peers?.map(peer => <option key={peer.instance_id} value={peer.instance_id}>For {accountNames.peers.get(peer.instance_id) ?? cleanMachineName(peer.instance_name, peer.instance_id) ?? "Machine"}{peer.online ? "" : " · offline"}</option>)}
          </select>
        </label>
        <div className="ct-appearance-lab__actions">
          <button type="button" disabled={!proposal?.can_undo || busy} onClick={() => void undo()}><Undo2 size={15} /> Undo</button>
          <button type="button" disabled={!current || busy} onClick={reset}>Reset</button>
          <button type="button" className="ct-appearance-primary" disabled={!generalTab || !current || busy} aria-expanded={ask} onClick={() => setAsk(value => !value)}><Sparkles size={15} /> Ask General</button>
        </div>
      </header>
      {ask && <form className="ct-appearance-ask" onSubmit={askGeneral}>
        <input className="ct-field ct-field-prominent" aria-label="Describe the look" value={prompt} maxLength={500} placeholder="Calm forest at night" onChange={event => setPrompt(event.target.value)} />
        <button className="ct-appearance-primary" type="submit" disabled={!prompt.trim() || !current}>Send to General</button>
      </form>}
      {unavailable && <p role="status">Offline. No changes applied to {machine}.</p>}
      {error && <p role="alert" className="ct-appearance-error">{error}</p>}
      {!draft && !unavailable && !error && <p role="status">Reading appearance…</p>}
      {current && draft && !unavailable && <>
        <div className="ct-appearance-lab__body">
          <div className="ct-appearance-lab__controls">
            <SettingsCard title="Presets" headerRight={<small>{current.presets.length} looks</small>}>
              <div className="ct-appearance-presets">
                {current.presets.map(theme => (
                  <button type="button" key={theme.name} onClick={() => pickPreset(theme)} aria-pressed={draft.theme.name === theme.name}
                    data-appearance-field="theme" style={{ background: theme.background, color: theme.foreground }}>
                    <span aria-hidden="true">{[theme.foreground, theme.blue, theme.green, theme.yellow, theme.red].map((colour, index) => <i key={index} style={{ background: colour }} />)}</span>
                    {theme.name}
                  </button>
                ))}
              </div>
            </SettingsCard>
            <SettingsCard title="Palette" headerRight={<small>Contrast against the background</small>}>
              {PALETTE_CONTROLS.map(({ key, label }) => {
                const ratio = paletteContrast(draft.theme, key);
                return <label key={key} className="ct-appearance-colour">
                  <input type="color" aria-label={label} data-appearance-field={`theme.${key}`} value={draft.theme[key]} onChange={event => editTheme(key, event.target.value)} />
                  <span>{label}</span><code>{draft.theme[key]}</code>
                  {ratio !== undefined && <output data-pass={ratio >= 4.5}>{ratio.toFixed(1)}:1 {ratio >= 4.5 ? "✓" : "✕"}</output>}
                </label>;
              })}
              <details className="ct-appearance-terminal">
                <summary>Terminal colours</summary>
                {TERMINAL_COLOURS.map(key => <label className="ct-appearance-colour" key={key}>
                  <input type="color" aria-label={key.replaceAll("_", " ")} data-appearance-field={`theme.${key}`} value={draft.theme[key]} onChange={event => editTheme(key, event.target.value)} />
                  <span>{key.replaceAll("_", " ")}</span><code>{draft.theme[key]}</code>
                </label>)}
              </details>
              {failures.length > 0 && <ul className="ct-appearance-contrast" aria-label="Contrast problems">
                {failures.map(failure => <li key={failure.pair}>
                  <span>{failure.pair} {failure.ratio.toFixed(2)}:1, needs {failure.floor}:1</span>
                  {failure.fix && <button type="button" onClick={() => setDraft(value => value && failure.fix ? applyContrastFix(value, failure.fix) : value)}
                    aria-label={`Fix ${failure.pair}`}>Fix</button>}
                </li>)}
              </ul>}
            </SettingsCard>
            <ChatWallpaperPicker config={config} showOwnInitially={showWallpaperOwn} stage={{
              theme: draft.theme, wallpaper: draft.wallpaper, peerId: target.peer, ready: !busy,
              update: wallpaper => setDraft(value => value && { ...value, wallpaper }),
            }} />
            <SettingsCard title="Shape and depth">
              <label className="ct-appearance-knob">Corner roundness
                <span><input type="range" data-appearance-field="appearance.roundness" min={0} max={1} step={0.05} value={tuning.roundness} onChange={event => editTuning({ roundness: event.target.valueAsNumber })} />
                  <span className="ct-appearance-ticks"><small>Square</small><small>Default</small></span>
                </span><output>{tuning.roundness.toFixed(2)}</output>
              </label>
              <label className="ct-appearance-knob">Shadow strength
                <input type="range" data-appearance-field="appearance.shadow_strength" min={0} max={1} step={0.05} value={tuning.shadow_strength} onChange={event => editTuning({ shadow_strength: event.target.valueAsNumber })} />
                <output>{tuning.shadow_strength === 1 ? "Default" : tuning.shadow_strength.toFixed(2)}</output>
              </label>
              <div className="ct-appearance-knob">Edges
                <div role="group" aria-label="Edges" className="ct-appearance-segment">
                  <button type="button" data-appearance-field="appearance.edges" aria-pressed={tuning.edges === "subtle"} onClick={() => editTuning({ edges: "subtle" })}>Subtle</button>
                  <button type="button" data-appearance-field="appearance.edges" aria-pressed={tuning.edges === "defined"} onClick={() => editTuning({ edges: "defined" })}>Defined</button>
                </div>
              </div>
            </SettingsCard>
            <SettingsCard title="Text">
              <div className="ct-appearance-knob">Interface text
                <div className="ct-appearance-size" role="group" aria-label="Interface text">
                  <button type="button" aria-label="Decrease interface text size" data-appearance-field="appearance.font_size" disabled={tuning.font_size <= -1} onClick={() => editTuning({ font_size: tuning.font_size - 1 })}>A−</button>
                  <output>{13 + tuning.font_size} px</output>
                  <button type="button" aria-label="Increase interface text size" data-appearance-field="appearance.font_size" disabled={tuning.font_size >= 2} onClick={() => editTuning({ font_size: tuning.font_size + 1 })}>A+</button>
                  {tuning.font_size === 0 && <small>Default</small>}
                </div>
              </div>
            </SettingsCard>
          </div>
          <div className="ct-appearance-lab__preview">
            <div className="ct-appearance-preview__header"><strong>Live preview</strong>
              {dirty && <div className="ct-appearance-segment" role="group" aria-label="Compare appearance">
                {(["before", "after", "side-by-side"] as const).map((mode, index) =>
                  <button key={mode} type="button" aria-pressed={compare === mode} onClick={() => setCompare(mode)}>{["Before", "After", "Side by side"][index]}</button>)}
              </div>}
            </div>
            <div className="ct-appearance-compare" data-compare={compare}>
              {compare !== "after" && <AppearancePreview look={current.look} peer={target.peer} />}
              {compare !== "before" && <AppearancePreview look={draft} peer={target.peer} />}
            </div>
            <button type="button" className="ct-appearance-primary" disabled={!dirty || busy || failures.length > 0} onClick={event => void apply(event.currentTarget)}>Apply to {machine}</button>
          </div>
        </div>
      </>}
    </section>
  );
}
