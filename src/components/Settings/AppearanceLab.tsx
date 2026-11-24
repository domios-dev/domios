import { useContext, useDeferredValue, useEffect, useId, useLayoutEffect, useRef, useState, type FormEvent } from "react";
import { Minus, Monitor, Sparkles, X } from "lucide-react";
import { DEFAULT_APPEARANCE, PALETTE_CONTROLS, TERMINAL_COLOURS, appearanceLookKey, applyContrastFix, paletteContrast, type AppearanceLook, type AppearanceProposal, type AppearanceTuning, type ContrastCheck, type CurrentAppearance } from "../../lib/appearance";
import { APPEARANCE_CHANGED, appearanceError, changeAppearance, checkAppearanceContrast, proposeAppearance, readAppearance, readAppearanceProposal, type AppearanceTarget, type OpenAppearance } from "../../lib/appearanceClient";
import { bindAppearancePress, clearAppearancePress, indicateAppearancePress } from "../../lib/appearanceMotion";
import { focusTab, generalAgentTab } from "../../lib/generalAgent";
import { useBackDismissal } from "../../lib/backDismissal";
import { setClientTabUiMode } from "../../lib/clientTabUiMode";
import { useMuxSelector } from "../../store/multiplexer/hooks";
import type { ExtendedState } from "../../store/multiplexer/tree";
import { sendTrackedChatMessage } from "../Terminal/chatSendStatus";
import { addChatSend } from "../Terminal/chatSendStore";
import { uid } from "../../lib/uid";
import { useAccountMachineNames, useMeshMachineChoices, useMeshPeerName } from "../../lib/useMeshMachineIdentities";
import { cleanMachineName } from "../../lib/machineIdentity";
import type { AppConfig, ThemeConfig } from "../../lib/types";
import { appearanceLabChanges, type AppearanceLabCategory, type AppearanceLabChange } from "./AppearanceLabChanges";
import { AppearanceLabDock, LAB_CATEGORIES } from "./AppearanceLabDock";
import type { AppearanceLabProjection } from "./AppearanceLabSession";
import { ChatWallpaperPicker } from "./ChatWallpaperPicker";
import { MachineAppearanceContext } from "../../lib/useMachineAppearance";
import { MultiplexerActionsContext, usePairedWorkspaceInventory } from "../../store/multiplexer/context";
import { relayTarget } from "../../lib/webApi";
import { SettingsCard } from "./shared";
import "./appearance.css";
import "./appearanceTry.css";

const mainTab = (state: ExtendedState) => generalAgentTab(state)?.tabId;
const mainWorkspace = (state: ExtendedState) => generalAgentTab(state)?.workspaceId;
const LOCAL: AppearanceTarget = { machine: "local" };
const APPLIED_TIME = new Intl.DateTimeFormat(undefined, { hour: "2-digit", minute: "2-digit", hourCycle: "h23" });

interface Props {
  config: AppConfig;
  open?: OpenAppearance;
  showWallpaperOwn?: boolean;
  onProject(projection?: AppearanceLabProjection): void;
  onClose(returnToOrigin: boolean): void;
}

export function AppearanceLab({ config, open, showWallpaperOwn, onProject, onClose }: Props) {
  const cardId = useId();
  const lab = useRef<HTMLElement>(null);
  const machineChoices = useMeshMachineChoices();
  const accountNames = useAccountMachineNames();
  const machineAppearance = useContext(MachineAppearanceContext);
  const viewedPeer = machineAppearance?.peerId === relayTarget()?.id ? undefined : machineAppearance?.peerId;
  const [target, setTarget] = useState<AppearanceTarget>(() => open ?? (viewedPeer ? { machine: viewedPeer, peer: viewedPeer } : LOCAL));
  const peerName = useMeshPeerName(target.peer);
  const [current, setCurrent] = useState<CurrentAppearance>();
  const [draft, setDraft] = useState<AppearanceLook>();
  const [openedProposal, setOpenedProposal] = useState<AppearanceProposal>();
  const wallpaperRevision = useRef(0);
  const [card, setCard] = useState<AppearanceLabCategory | undefined>(showWallpaperOwn ? "wallpaper" : "presets");
  const [collapsed, setCollapsed] = useState(false);
  const [listOpen, setListOpen] = useState(false);
  const [comparing, setComparing] = useState(false);
  const [justApplied, setJustApplied] = useState<AppearanceProposal>();
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
  const appearanceOnline = !remote || machineChoices.peers?.some(peer => peer.instance_id === target.peer && peer.online);
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
      setOpenedProposal(stored);
    }, (failure: unknown) => {
      if (active) setError(appearanceError(failure));
    });
    return () => { active = false; };
  }, [target, open, appearanceOnline]);

  useEffect(() => {
    const onChange = (event: Event) => {
      const detail = (event as CustomEvent<{ receipt: AppearanceProposal }>).detail;
      if (detail?.receipt.machine.id !== current?.machine.id) return;
      wallpaperRevision.current++;
      setOpenedProposal(undefined);
      const look = detail.receipt.state === "undone" ? detail.receipt.previous : detail.receipt.look;
      setCurrent(previous => previous && { ...previous, look, active: detail.receipt.can_undo ? detail.receipt : null });
      setDraft(look);
    };
    window.addEventListener(APPEARANCE_CHANGED, onChange);
    return () => window.removeEventListener(APPEARANCE_CHANGED, onChange);
  }, [current?.machine.id]);

  const changeMachine = (id: string) => {
    wallpaperRevision.current++;
    onProject(undefined);
    setCurrent(undefined);
    setDraft(undefined);
    setOpenedProposal(undefined);
    setJustApplied(undefined);
    setComparing(false);
    setListOpen(false);
    setError("");
    setTarget(id === "local" ? LOCAL : { machine: id, peer: id });
  };
  const currentName = cleanMachineName(current?.machine.name, current?.machine.id);
  const localName = accountNames.local ?? machineChoices.local_instance_name ?? (!target.peer ? currentName : undefined);
  const machine = target.peer ? peerName ?? currentName ?? "Machine" : localName ?? "this machine";
  const unavailable = Boolean(target.peer && machineChoices.peers?.find(peer => peer.instance_id === target.peer)?.online === false);
  const tuning = draft?.tuning ?? DEFAULT_APPEARANCE;
  const changes = current && draft ? appearanceLabChanges(current.look, draft, current.presets) : [];
  const dirty = changes.length > 0;
  const active = justApplied?.can_undo ? justApplied : undefined;
  const undoLabel = active && `Undo · back to ${active.previous.theme.name}`;
  const appliedAt = active?.applied_at == null ? undefined : new Date(active.applied_at * 1000);
  const wallpaperVersion = wallpaperRevision.current;
  const visibleCard = collapsed || justApplied ? undefined : card;
  const projectingCommit = Boolean(justApplied && (target.peer ?? relayTarget()?.id) === machineAppearance?.peerId);
  const projectedLook = unavailable ? undefined : justApplied ? (projectingCommit ? justApplied.look : undefined) : comparing ? current?.look : draft;
  useLayoutEffect(() => {
    onProject(projectedLook && { look: projectedLook, peer: target.peer, committed: projectingCommit });
  }, [onProject, projectedLook, target.peer, projectingCommit]);
  useEffect(() => {
    if (!justApplied) return;
    const timeout = window.setTimeout(() => onClose(false), 8_000);
    return () => window.clearTimeout(timeout);
  }, [justApplied, onClose]);
  const focusCategory = (category: AppearanceLabCategory) =>
    lab.current?.querySelector<HTMLButtonElement>(`[data-lab-category="${category}"]`)?.focus();
  const dismissCard = (minimize: boolean) => {
    if (card) focusCategory(card);
    setCollapsed(minimize);
    if (!minimize) setCard(undefined);
  };
  const dismiss = () => {
    if (busy) return;
    if (visibleCard) dismissCard(false);
    else onClose(true);
  };
  const dismissRef = useRef(dismiss);
  dismissRef.current = dismiss;
  useBackDismissal(true, dismiss);
  useEffect(() => {
    const escape = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      event.stopImmediatePropagation();
      dismissRef.current();
    };
    window.addEventListener("keydown", escape, true);
    return () => window.removeEventListener("keydown", escape, true);
  }, []);
  const selectCategory = (category: AppearanceLabCategory) => {
    if (visibleCard === category) dismissCard(true);
    else {
      setCard(category);
      setCollapsed(false);
      setListOpen(false);
    }
  };
  const failures = contrast?.look === draft ? contrast?.checks.filter(check => check.ratio < check.floor) ?? [] : [];
  const editTuning = (patch: Partial<AppearanceTuning>) => setDraft(value => {
    if (!value) return value;
    const next = { ...value.tuning ?? DEFAULT_APPEARANCE, ...patch };
    const baseline = current?.look.tuning ?? DEFAULT_APPEARANCE;
    const matchesCurrent = (Object.keys(next) as (keyof AppearanceTuning)[]).every(key => next[key] === baseline[key]);
    return { ...value, tuning: matchesCurrent ? current?.look.tuning ?? null : next };
  });
  const editTheme = (key: keyof ThemeConfig, colour: string) => setDraft(value => value && { ...value, theme: { ...value.theme, [key]: colour } });

  const apply = async (button: HTMLButtonElement) => {
    if (!draft || !current || !dirty || busy || failures.length > 0) return;
    indicateAppearancePress(button);
    setBusy(true);
    setError("");
    try {
      const next = openedProposal && openedProposal.state !== "applied"
        && appearanceLookKey(draft) === appearanceLookKey(openedProposal.look)
        && appearanceLookKey(current.look) === appearanceLookKey(openedProposal.previous)
        ? openedProposal : await proposeAppearance(target, draft, current.look);
      bindAppearancePress(button, next.id, next.look, target.peer ?? relayTarget()?.id);
      const receipt = await changeAppearance(target, next.id, "apply");
      if (receipt.can_undo) {
        setJustApplied(receipt);
        setListOpen(false);
      }
    } catch (failure) {
      clearAppearancePress(button);
      setError(appearanceError(failure));
    } finally {
      setBusy(false);
    }
  };

  const undo = async () => {
    if (!active || dirty || busy) return;
    setBusy(true);
    setError("");
    try {
      await changeAppearance(target, active.id, "undo");
      if (justApplied) onClose(false);
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
    const request = `Appearance request for ${name}: ${text}\n\nCreate a proposal for me to review in Appearance Lab. Keep the current appearance until I choose Apply.\n\n<context>\nAppearance Lab · for ${name}\nInstallation: ${current.machine.id}\nCurrent look: ${JSON.stringify(look)}\nSee domios docs APPEARANCE for tweaks and proposals.\n</context>`;
    setClientTabUiMode(generalTab, "chat");
    const id = uid();
    addChatSend(generalTab, { id, correlationId: id, content: request, sentAtMs: Date.now() });
    const sent = sendTrackedChatMessage(generalTab, id, request);
    onClose(false);
    if (remoteGeneral && remoteMain && workspaceId && actions?.attachRemoteTab) {
      actions.attachRemoteTab(workspaceId, { peer_id: remoteGeneral.peerId, peer_name: remoteGeneral.name, tabs: [remoteMain.tab] }, remoteMain.tab, remoteMain.status);
    } else {
      focusTab(generalTab, workspaceId ?? null);
    }
    void sent.catch(() => {});
  };

  const pickPreset = (theme: ThemeConfig) => setDraft(value => value && { ...value, theme });
  const discardWallpaper = () => {
    if (!current) return;
    wallpaperRevision.current++;
    setDraft(look => look && { ...look, wallpaper: current.look.wallpaper,
      theme: { ...look.theme, chat_background: current.look.theme.chat_background } });
  };
  const revert = (change: AppearanceLabChange) => {
    if (change.category === "wallpaper") wallpaperRevision.current++;
    setDraft(look => look && change.revert(look));
  };

  const selectedCategory = LAB_CATEGORIES.find(item => item.id === card);
  const cardTitle = card === "shape" ? "Shape and depth" : selectedCategory?.label;
  const machineSelector = (
    <label className="ct-appearance-machine"><Monitor size={15} aria-hidden="true" />For:
      <select aria-label="Appearance machine" value={target.peer ?? "local"} onChange={event => changeMachine(event.target.value)} disabled={busy}>
        <option value="local">{localName ? `This machine · ${localName}` : "This machine"}</option>
        {machineChoices.peers?.map(peer => <option key={peer.instance_id} value={peer.instance_id}>{accountNames.peers.get(peer.instance_id) ?? cleanMachineName(peer.instance_name, peer.instance_id) ?? "Machine"}{peer.online ? "" : " · offline"}</option>)}
      </select>
    </label>
  );
  return (
    <section ref={lab} className="ct-appearance-lab ct-appearance-try" aria-label="Appearance Lab"
      data-card-open={visibleCard ? "" : undefined} data-applied={justApplied ? "" : undefined}>
      {projectedLook && !justApplied && <p className="ct-appearance-try__tag" role="status">
        {comparing ? "Comparing · current look" : "● Previewing · not applied"}<span> · Esc to exit</span>
      </p>}
      {unavailable && <p className="ct-appearance-try__notice" role="status">Offline. No changes applied to {machine}.</p>}
      {error && <p role="alert" className="ct-appearance-try__notice ct-appearance-error">{error}</p>}
      {!draft && !unavailable && !error && <p className="ct-appearance-try__notice" role="status">Reading appearance…</p>}
      {collapsed && card && selectedCategory && !justApplied && <button type="button" className="ct-appearance-try__pill"
        onClick={() => selectCategory(card)}><selectedCategory.icon size={15} aria-hidden="true" />
        {selectedCategory.label} · {changes.filter(change => change.category === card).length} changes</button>}
      <div id={cardId} className="ct-appearance-try__card" role="region" aria-label={cardTitle ?? "Appearance controls"} hidden={!visibleCard}>
        <header>
          <h2>{cardTitle}</h2>
          <button type="button" aria-label="Collapse appearance card" onClick={() => dismissCard(true)}><Minus size={15} aria-hidden="true" /></button>
          <button type="button" aria-label="Close appearance card" onClick={() => dismissCard(false)}><X size={15} aria-hidden="true" /></button>
        </header>
        {current && draft && !unavailable && visibleCard && <fieldset disabled={busy} className="ct-appearance-try__controls" aria-label="Appearance controls">
          {visibleCard === "presets" && (
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
              <button type="button" className="ct-appearance-lab__ask" disabled={!generalTab || busy} aria-expanded={ask}
                onClick={() => setAsk(value => !value)}><Sparkles size={15} aria-hidden="true" /> Ask General for a look</button>
              {ask && <form className="ct-appearance-ask" onSubmit={askGeneral}>
                <input className="ct-field ct-field-prominent" aria-label="Describe the look" value={prompt} maxLength={500}
                  placeholder="Calm forest at night" onChange={event => setPrompt(event.target.value)} />
                <button type="submit" disabled={!prompt.trim() || busy}>Send to General</button>
              </form>}
            </SettingsCard>
          )}
          {visibleCard === "palette" && (
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
                <summary>Terminal colours · {TERMINAL_COLOURS.length}</summary>
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
          )}
          {visibleCard === "wallpaper" && (
            <ChatWallpaperPicker key={wallpaperVersion} config={config} showOwnInitially={showWallpaperOwn && wallpaperVersion === 0} stage={{
              theme: draft.theme, wallpaper: draft.wallpaper, peerId: target.peer, ready: !busy,
              discard: discardWallpaper,
              ask: generalTab ? () => { setAsk(true); setCard("presets"); } : undefined,
              update: wallpaper => {
                if (wallpaperVersion === wallpaperRevision.current) setDraft(value => value && { ...value, wallpaper });
              },
            }} />
          )}
          {visibleCard === "shape" && (
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
              <label className="ct-appearance-knob">Border brightness
                <input type="range" aria-label="Border brightness" data-appearance-field="appearance.border_strength" min={0} max={1} step={0.01} value={tuning.border_strength} onChange={event => editTuning({ border_strength: event.target.valueAsNumber })} />
                <output>{Math.round(tuning.border_strength * 100)}%</output>
              </label>
            </SettingsCard>
          )}
          {visibleCard === "text" && (
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
          )}
        </fieldset>}
      </div>
      <AppearanceLabDock cardId={cardId} category={visibleCard} changes={changes} listOpen={listOpen} busy={busy}
        canApply={dirty && !busy && !comparing && !unavailable && failures.length === 0}
        comparing={comparing} applyLabel={remote ? `Apply to ${machine}` : "Apply"}
        appliedStatus={justApplied && <span>Applied{appliedAt && <> · <time dateTime={appliedAt.toISOString()}>{APPLIED_TIME.format(appliedAt)}</time></>}</span>}
        undoLabel={dirty || unavailable ? undefined : undoLabel} machineSelector={machineSelector}
        onCategory={selectCategory} onToggleList={() => setListOpen(value => !value)} onCompare={setComparing}
        onRevert={revert} onDiscard={() => onClose(true)} onApply={button => void apply(button)} onUndo={() => void undo()}
        onBack={() => onClose(true)} />
    </section>
  );
}
