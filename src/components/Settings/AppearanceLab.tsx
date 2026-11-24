import { useContext, useEffect, useId, useLayoutEffect, useMemo, useRef, useState, type FormEvent, type RefObject } from "react";
import { Monitor, Sparkles } from "lucide-react";
import { usePhoneViewport } from "../../hooks/useSidebarViewport";
import { DEFAULT_APPEARANCE, PALETTE_CONTROLS, TERMINAL_COLOURS, appearanceLookKey, applyContrastFix, paletteContrast, trustedPreset, type AppearanceLook, type AppearanceProposal, type AppearanceTuning, type ContrastCheck, type CurrentAppearance } from "../../lib/appearance";
import { APPEARANCE_CHANGED, appearanceError, changeAppearance, previewAppearancePreset, proposeAppearance, readAppearance, readAppearanceProposal, switchAppearanceTheme, type AppearanceTarget, type OpenAppearance } from "../../lib/appearanceClient";
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
import { appearanceLabChanges, type AppearanceLabCategory, type AppearanceLabChange } from "./AppearanceLabChanges";
import { AppearanceAppliedLabel, AppearanceLabActions, AppearanceLabCategories, LAB_CATEGORIES, contrastIssues } from "./AppearanceLabPanelControls";
import { useAppearanceLabDismissal, type AppearanceLabProjection } from "./AppearanceLabSession";
import type { AppearanceLabHostProps } from "./AppearanceLabHost";
import { AppearancePaletteInput, useAppearancePalette } from "./AppearancePaletteInput";
import { useAppearanceContrast } from "./useAppearanceContrast";
import { usePresence, type PresenceState } from "../Canvas/usePresence";
import { TabSidePanel } from "../Terminal/tabPanel/TabSidePanel";
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
const SIDE_PANEL_MOTION = { enter: "ct-plan-side-panel-in", exit: "ct-plan-side-panel-out" };

export function AppearanceLabLayer({ config, session, onProject, onClose, onExited }: AppearanceLabHostProps & { onExited(): void }) {
  const opener = useRef<HTMLElement | undefined>(undefined);
  if (session) opener.current = session.opener;
  const presence = usePresence(session ?? null, SIDE_PANEL_MOTION, panel => {
    const active = document.activeElement;
    if (opener.current?.isConnected && (!active || active === document.body || panel?.contains(active))) {
      opener.current.focus({ preventScroll: true });
    }
    opener.current = undefined;
  });
  useEffect(() => {
    if (!presence.item) onExited();
  }, [presence.item, onExited]);
  if (!presence.item || !presence.state) return null;
  return <AppearanceLab key={presence.item.id} config={config} open={presence.item.proposal}
    showWallpaperOwn={presence.item.showWallpaper} state={presence.state} panelRef={presence.ref}
    onProject={onProject} onClose={onClose} />;
}

interface Props {
  config: AppConfig;
  open?: OpenAppearance;
  showWallpaperOwn?: boolean;
  state: PresenceState;
  panelRef: RefObject<HTMLElement | null>;
  onProject(projection?: AppearanceLabProjection): void;
  onClose(returnToOrigin: boolean): void;
}

function AppearanceLab({ config, open, showWallpaperOwn, state, panelRef, onProject, onClose }: Props) {
  const controlsId = useId();
  const changesTrigger = useRef<HTMLButtonElement>(null);
  const machineChoices = useMeshMachineChoices();
  const accountNames = useAccountMachineNames();
  const machineAppearance = useContext(MachineAppearanceContext);
  const viewedPeer = machineAppearance?.peerId === relayTarget()?.id ? undefined : machineAppearance?.peerId;
  const [target, setTarget] = useState<AppearanceTarget>(() => open ?? (viewedPeer ? { machine: viewedPeer, peer: viewedPeer } : LOCAL));
  const peerName = useMeshPeerName(target.peer);
  const [current, setCurrent] = useState<CurrentAppearance>();
  const [draft, setDraft] = useState<AppearanceLook>();
  const { editing: paletteEditing, editColour, editDraft, commit: commitColour, reset: resetPalette } = useAppearancePalette(setDraft);
  const [settledDraft, setSettledDraft] = useState(draft);
  if (!paletteEditing && settledDraft !== draft) setSettledDraft(draft);
  const [openedProposal, setOpenedProposal] = useState<AppearanceProposal>();
  const [presetPreview, setPresetPreview] = useState<AppearanceLook>();
  const wallpaperRevision = useRef(0);
  const [category, setCategory] = useState<AppearanceLabCategory>(showWallpaperOwn ? "wallpaper" : "presets");
  const phone = usePhoneViewport();
  const [menuOpen, setMenuOpen] = useState(false);
  const [changesExpanded, setChangesExpanded] = useState<boolean>();
  const [comparing, setComparing] = useState(false);
  const [justApplied, setJustApplied] = useState<AppearanceProposal>();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [ask, setAsk] = useState(false);
  const [prompt, setPrompt] = useState("");
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
  const audit = useAppearanceContrast(target, draft, Boolean(appearanceOnline) && state !== "closing" && !paletteEditing);

  useEffect(() => {
    let active = true;
    if (!appearanceOnline) return;
    const receipt = open?.proposal && open.machine === target.machine && open.peer === target.peer
      ? readAppearanceProposal(target, open.proposal) : undefined;
    Promise.all([readAppearance(target), receipt]).then(([next, stored]) => {
      if (!active) return;
      resetPalette();
      setCurrent(next);
      setDraft(stored?.look ?? next.look);
      setOpenedProposal(stored);
      setPresetPreview(undefined);
    }, (failure: unknown) => {
      if (active) setError(appearanceError(failure));
    });
    return () => { active = false; };
  }, [target, open, appearanceOnline, resetPalette]);

  useEffect(() => {
    const onChange = (event: Event) => {
      const detail = (event as CustomEvent<{ receipt: AppearanceProposal }>).detail;
      if (detail?.receipt.machine.id !== current?.machine.id) return;
      resetPalette();
      wallpaperRevision.current++;
      setOpenedProposal(undefined);
      setPresetPreview(undefined);
      const look = detail.receipt.state === "undone" ? detail.receipt.previous : detail.receipt.look;
      setCurrent(previous => previous && { ...previous, look, active: detail.receipt.can_undo ? detail.receipt : null });
      setDraft(look);
    };
    window.addEventListener(APPEARANCE_CHANGED, onChange);
    return () => window.removeEventListener(APPEARANCE_CHANGED, onChange);
  }, [current?.machine.id, resetPalette]);

  const changeMachine = (id: string) => {
    resetPalette();
    wallpaperRevision.current++;
    onProject(undefined);
    setCurrent(undefined);
    setDraft(undefined);
    setOpenedProposal(undefined);
    setPresetPreview(undefined);
    setJustApplied(undefined);
    setComparing(false);
    setChangesExpanded(undefined);
    setError("");
    setTarget(id === "local" ? LOCAL : { machine: id, peer: id });
  };
  const currentName = cleanMachineName(current?.machine.name, current?.machine.id);
  const localName = accountNames.local ?? machineChoices.local_instance_name ?? (!target.peer ? currentName : undefined);
  const machine = target.peer ? peerName ?? currentName ?? "Machine" : localName ?? "this machine";
  const unavailable = Boolean(target.peer && machineChoices.peers?.find(peer => peer.instance_id === target.peer)?.online === false);
  const tuning = draft?.tuning ?? DEFAULT_APPEARANCE;
  const changes = useMemo(() => current && settledDraft ? appearanceLabChanges(current.look, settledDraft, current.presets) : [],
    [current, settledDraft]);
  const dirty = changes.length > 0;
  const active = justApplied?.can_undo ? justApplied : undefined;
  const undoLabel = active && `Undo · back to ${active.previous.theme.name}`;
  const wallpaperVersion = wallpaperRevision.current;
  const closing = state === "closing";
  const projectingCommit = Boolean(justApplied && (target.peer ?? relayTarget()?.id) === machineAppearance?.peerId);
  const projectedLook = unavailable ? undefined : justApplied ? (projectingCommit ? justApplied.look : undefined) : comparing ? current?.look : draft;
  useLayoutEffect(() => {
    if (!closing) onProject(projectedLook && { look: projectedLook, peer: target.peer, committed: projectingCommit });
  }, [onProject, projectedLook, target.peer, projectingCommit, closing]);
  const dismiss = () => {
    if (!closing && !busy) {
      resetPalette();
      onClose(!justApplied);
    }
  };
  const editing = !closing && !justApplied;
  const showChanges = editing && dirty && (changesExpanded ?? !phone);
  const activePopover = !editing ? undefined : menuOpen ? "menu" : showChanges && changesExpanded ? "changes" : undefined;
  useAppearanceLabDismissal(!closing, activePopover, changesTrigger, () => setChangesExpanded(false), dismiss);
  const toggleMenu = (open: boolean) => {
    setMenuOpen(open);
    if (!open) setComparing(false);
  };
  const selectCategory = (next: AppearanceLabCategory) => {
    setCategory(next);
    toggleMenu(false);
  };
  const { failures } = audit;
  const isTrustedPreset = trustedPreset(draft, presetPreview) && !paletteEditing;
  const issues = useMemo(() => dirty && !isTrustedPreset ? contrastIssues(failures) : [], [dirty, failures, isTrustedPreset]);
  const ratioTheme = audit.theme ?? current?.look.theme;
  const ratios = useMemo(() => new Map(PALETTE_CONTROLS.map(({ key }) =>
    [key, ratioTheme && paletteContrast(ratioTheme, key)])), [ratioTheme]);
  const canApply = dirty && !busy && !comparing && !unavailable &&
    (isTrustedPreset || (audit.ready && failures.length === 0));
  const fixContrast = (fix: NonNullable<ContrastCheck["fix"]>) => {
    if (audit.ready) editDraft(value => value && applyContrastFix(value, fix));
  };
  const fixAllContrast = () => {
    if (audit.ready) editDraft(value => issues.reduce((look, issue) => look && issue.fix ? applyContrastFix(look, issue.fix) : look, value));
  };
  const editTuning = (patch: Partial<AppearanceTuning>) => editDraft(value => {
    if (!value) return value;
    const next = { ...value.tuning ?? DEFAULT_APPEARANCE, ...patch };
    const baseline = current?.look.tuning ?? DEFAULT_APPEARANCE;
    const matchesCurrent = (Object.keys(next) as (keyof AppearanceTuning)[]).every(key => next[key] === baseline[key]);
    return { ...value, tuning: matchesCurrent ? current?.look.tuning ?? null : next };
  });
  const apply = async (button: HTMLButtonElement) => {
    if (!draft || !current || !canApply) return;
    indicateAppearancePress(button);
    setBusy(true);
    setError("");
    try {
      let receipt: AppearanceProposal | null;
      if (isTrustedPreset) {
        bindAppearancePress(button, null, draft, target.peer ?? relayTarget()?.id);
        receipt = await switchAppearanceTheme(target, { kind: "preset", name: draft.theme.name, expected: current.look });
        if (!receipt) clearAppearancePress(button);
      } else {
        const next = openedProposal && openedProposal.state !== "applied"
          && appearanceLookKey(draft) === appearanceLookKey(openedProposal.look)
          && appearanceLookKey(current.look) === appearanceLookKey(openedProposal.previous)
          ? openedProposal : await proposeAppearance(target, draft, current.look);
        bindAppearancePress(button, next.id, next.look, target.peer ?? relayTarget()?.id);
        receipt = await changeAppearance(target, next.id, "apply");
      }
      if (receipt?.can_undo) {
        if (panelRef.current?.contains(document.activeElement)) panelRef.current.focus();
        setJustApplied(receipt);
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

  const pickPreset = async (theme: ThemeConfig) => {
    if (!current || busy) return;
    const revision = ++wallpaperRevision.current;
    setBusy(true);
    setError("");
    try {
      const preview = await previewAppearancePreset(target, theme.name, current.look);
      if (revision !== wallpaperRevision.current) return;
      setPresetPreview(preview);
      editDraft(value => {
        if (!value) return value;
        const wallpaperKey = (look: AppearanceLook) => appearanceLookKey({ ...current.look, wallpaper: look.wallpaper });
        const existing = wallpaperKey(value);
        const keepEdit = existing !== wallpaperKey(current.look) && (!presetPreview || existing !== wallpaperKey(presetPreview));
        return { ...value, theme: preview.theme, wallpaper: keepEdit ? value.wallpaper : preview.wallpaper };
      });
    } catch (failure) {
      if (revision === wallpaperRevision.current) setError(appearanceError(failure));
    } finally {
      setBusy(false);
    }
  };
  const discardWallpaper = () => {
    if (!current) return;
    wallpaperRevision.current++;
    editDraft(look => look && { ...look, wallpaper: current.look.wallpaper,
      theme: { ...look.theme, chat_background: current.look.theme.chat_background } });
  };
  const revert = (change: AppearanceLabChange) => {
    if (change.category === "wallpaper") wallpaperRevision.current++;
    if (changes.length === 1) {
      setChangesExpanded(undefined);
      panelRef.current?.focus();
    }
    editDraft(look => look && change.revert(look));
  };

  const controlsLabel = category === "shape" ? "Shape and depth" : LAB_CATEGORIES.find(item => item.id === category)?.label;
  const machineSelector = (
    <label className="ct-appearance-machine"><Monitor size={15} aria-hidden="true" />For:
      <select autoFocus aria-label="Appearance machine" value={target.peer ?? "local"} onChange={event => changeMachine(event.target.value)} disabled={busy}>
        <option value="local">{localName ? `This machine · ${localName}` : "This machine"}</option>
        {machineChoices.peers?.map(peer => <option key={peer.instance_id} value={peer.instance_id}>{accountNames.peers.get(peer.instance_id) ?? cleanMachineName(peer.instance_name, peer.instance_id) ?? "Machine"}{peer.online ? "" : " · offline"}</option>)}
      </select>
    </label>
  );
  const previewTag = projectedLook && !closing && !justApplied && <p className="ct-appearance-try__tag" role="status">
    {comparing ? "Comparing" : "Previewing"}<span>{comparing ? " · current look" : " · not applied"} · Esc to exit</span>
  </p>;
  const controls = (
    <div id={controlsId} className="ct-appearance-lab" role="region" aria-label={controlsLabel}>
      {current && draft && !unavailable && <fieldset disabled={busy} className="ct-appearance-try__controls" aria-label="Appearance controls">
        {category === "presets" && (
          <SettingsCard title="Presets" headerRight={<small>{current.presets.length} looks</small>}>
            <div className="ct-appearance-presets">
              {current.presets.map(theme => (
                <button type="button" key={theme.name} onClick={() => void pickPreset(theme)} aria-pressed={draft.theme.name === theme.name}
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
        {category === "palette" && (
          <SettingsCard title="Palette" headerRight={<small>Contrast against the background</small>}>
            {PALETTE_CONTROLS.map(({ key, label }) => {
              const field = `theme.${key}`;
              const failing = failures.filter(failure => failure.fix?.field === field);
              const worst = failing.length ? failing.reduce((low, check) => check.ratio < low.ratio ? check : low) : undefined;
              const ratio = worst?.ratio ?? ratios.get(key);
              const pass = worst ? false : ratio !== undefined && ratio >= 4.5;
              const fix = worst?.fix;
              return <div key={key} className="ct-appearance-colour">
                <AppearancePaletteInput id={`${controlsId}-${key}`} label={label} field={field} colour={draft.theme[key]}
                  editing={paletteEditing} onEdit={colour => editColour(key, colour)} onCommit={commitColour} />
                <label htmlFor={`${controlsId}-${key}`}>{label}</label><code>{draft.theme[key]}</code>
                <span className="ct-appearance-colour__status">
                  {ratio !== undefined && <output data-pass={pass}
                    title={worst ? `${worst.pair} ${worst.ratio.toFixed(2)}:1, needs ${worst.floor}:1${failing.length > 1 ? ` · fails on ${failing.length} surfaces` : ""}` : undefined}>
                    {ratio.toFixed(1)}:1{pass ? " ✓" : ""}</output>}
                  {fix && <button type="button" aria-label={`Fix ${label}`} disabled={!audit.ready} onClick={() => fixContrast(fix)}>Fix</button>}
                </span>
              </div>;
            })}
            <details className="ct-appearance-terminal">
              <summary>Terminal colours · {TERMINAL_COLOURS.length}</summary>
              {TERMINAL_COLOURS.map(key => <label className="ct-appearance-colour" key={key}>
                <AppearancePaletteInput label={key.replaceAll("_", " ")} field={`theme.${key}`} colour={draft.theme[key]}
                  editing={paletteEditing} onEdit={colour => editColour(key, colour)} onCommit={commitColour} />
                <span>{key.replaceAll("_", " ")}</span><code>{draft.theme[key]}</code>
              </label>)}
            </details>
          </SettingsCard>
        )}
        {category === "wallpaper" && (
          <ChatWallpaperPicker key={wallpaperVersion} config={config} showOwnInitially={showWallpaperOwn && wallpaperVersion === 0} stage={{
            theme: draft.theme, wallpaper: draft.wallpaper, peerId: target.peer, ready: !busy,
            discard: discardWallpaper,
            ask: generalTab ? () => { setAsk(true); selectCategory("presets"); } : undefined,
            update: wallpaper => {
              if (wallpaperVersion === wallpaperRevision.current) editDraft(value => value && { ...value, wallpaper });
            },
          }} />
        )}
        {category === "shape" && (
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
        {category === "text" && (
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
  );
  return (
    <div className="plan-side-panel-layer ct-appearance-try">
      <TabSidePanel panelRef={panelRef} state={state} label="Appearance Lab" onClose={dismiss}
        data-appearance-panel="" title={<h2 className="plan-side-panel__title">Appearance Lab</h2>} actions={previewTag}
        toolbar={!justApplied && <AppearanceLabCategories controlsId={controlsId} category={category} changes={changes} busy={busy}
          comparing={comparing} menuOpen={activePopover === "menu"} machineSelector={machineSelector}
          onCategory={selectCategory} onMenuChange={toggleMenu}
          onCompare={setComparing} onBack={dismiss} />}
        footer={<AppearanceLabActions changes={changes} changesTrigger={changesTrigger} listOpen={showChanges}
          focusChanges={changesExpanded === true} busy={busy}
          canApply={canApply}
          applyLabel={remote ? `Apply to ${machine}` : "Apply"}
          appliedStatus={justApplied && <AppearanceAppliedLabel receipt={justApplied} />}
          undoLabel={dirty || unavailable ? undefined : undoLabel}
          contrast={issues} canFixContrast={audit.ready} onFixContrast={fixContrast} onFixAllContrast={fixAllContrast}
          onToggleList={() => { toggleMenu(false); setChangesExpanded(!showChanges); }}
          onRevert={revert} onDiscard={dismiss} onApply={button => void apply(button)} onUndo={() => void undo()} />}>
        {unavailable && <p className="ct-appearance-try__notice" role="status">Offline. No changes applied to {machine}.</p>}
        {error && <p role="alert" className="ct-appearance-try__notice ct-appearance-error">{error}</p>}
        {dirty && !isTrustedPreset && audit.error && <p role="alert" className="ct-appearance-try__notice ct-appearance-error">
          {audit.error} <button type="button" onClick={audit.retry}>Retry contrast check</button>
        </p>}
        {!draft && !unavailable && !error && <p className="ct-appearance-try__notice" role="status">Reading appearance…</p>}
        {!justApplied && controls}
      </TabSidePanel>
    </div>
  );
}
