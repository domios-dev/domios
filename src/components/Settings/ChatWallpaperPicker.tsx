import { useContext, useId, useRef, useState, type CSSProperties } from "react";
import { Check, Plus, Upload, Wallpaper } from "lucide-react";
import type { ChatWallpaper } from "../../../gen/ChatWallpaper";
import { formatBytes } from "../../lib/formatBytes";
import { isLightColor, DEFAULT_THEME, themeToCSS } from "../../lib/themeCss";
import { MachineAppearanceContext, useWallpaperAsset } from "../../lib/useMachineAppearance";
import { effectiveWallpaper, WALLPAPER_PATTERNS, wallpaperRenderStyle, wallpaperThumbnailIntensity } from "../../lib/chatWallpaper";
import { askGeneralForWallpaper, deleteWallpaperAsset, saveWallpaper, uploadWallpaper, WALLPAPER_FILE_TYPES } from "../../lib/chatWallpaperClient";
import type { AppConfig, ThemeConfig } from "../../lib/types";
import { UI_BUTTON, UI_INPUT } from "../shared/design";
import { SettingsCard } from "./shared";
import { WallpaperControls } from "./WallpaperControls";

const lightTheme: ThemeConfig = {
  ...DEFAULT_THEME, name: "github-light", background: "#f6f8fa", foreground: "#1f2328", blue: "#0969da",
};

interface StagedWallpaper {
  theme: ThemeConfig;
  wallpaper: ChatWallpaper | null;
  peerId?: string;
  ready: boolean;
  update(wallpaper: ChatWallpaper | null): void;
  discard(): void;
  ask?(): void;
}

function thumbnailStyle(theme: ThemeConfig, wallpaper: ChatWallpaper, light: boolean, scale: number, assetUrl?: string) {
  const thumbnail = { ...wallpaper, background: { ...wallpaper.background, scale } };
  return wallpaperRenderStyle(theme, thumbnail, light, assetUrl, undefined, wallpaperThumbnailIntensity(light));
}

export function ChatWallpaperPicker({ config, showOwnInitially, stage }: { config: AppConfig; showOwnInitially?: boolean; stage?: StagedWallpaper }) {
  const selected = useContext(MachineAppearanceContext);
  if (stage) return <Picker key={stage.peerId ?? "local"} theme={stage.theme} installed={stage.wallpaper}
    peerId={stage.peerId} ready={stage.ready} stage={stage.update} discard={stage.discard} ask={stage.ask} showOwnInitially={showOwnInitially} />;
  const appearance = selected?.appearance ?? { theme: config.theme, wallpaper: config.chat_wallpaper ?? null };
  return <Picker key={`${selected?.peerId ?? "local"}:${appearance.theme.name}`} theme={appearance.theme} installed={appearance.wallpaper}
    machineColor={selected?.identity?.color} peerId={selected?.peerId} ready={selected?.ready ?? true}
    project={selected?.projectWallpaper} showOwnInitially={showOwnInitially} />;
}

interface PickerProps {
  theme: ThemeConfig;
  installed: ChatWallpaper | null;
  machineColor?: Parameters<typeof wallpaperRenderStyle>[4];
  peerId?: string;
  ready: boolean;
  project?: (wallpaper: ChatWallpaper | null) => void;
  showOwnInitially?: boolean;
  stage?: StagedWallpaper["update"];
  discard?: StagedWallpaper["discard"];
  ask?: StagedWallpaper["ask"];
}

function Picker({ theme, installed, machineColor, peerId, ready, project, showOwnInitially, stage, discard, ask }: PickerProps) {
  const themeIsLight = isLightColor(theme.background);
  const [lightOverride, setLightOverride] = useState<boolean>();
  const light = lightOverride ?? themeIsLight;
  const [ownOpen, setOwnOpen] = useState(Boolean(showOwnInitially));
  const [description, setDescription] = useState("");
  const [busy, setBusy] = useState(false);
  const busyRef = useRef(false);
  const [request, setRequest] = useState<{ previous: ChatWallpaper | null }>();
  const [error, setError] = useState<{ reason: string; file?: string }>();
  const [edit, setEdit] = useState<{ base: ChatWallpaper | null; value: ChatWallpaper | null }>();
  const source = edit && edit.base === installed ? edit.value : installed;
  const active = effectiveWallpaper(theme, source ?? undefined);
  const custom = active.background.pattern === "svg" || active.background.pattern === "image";
  const showOwn = ownOpen || (custom && Boolean(source));
  const asset = useWallpaperAsset(peerId, ready ? active.asset : undefined);
  const hasOwnFile = custom && Boolean(active.asset);
  const pickerInput = useRef<HTMLInputElement>(null);
  const group = useId();

  async function run(action: () => Promise<void>, file?: string) {
    if (!ready || busyRef.current) return;
    busyRef.current = true;
    setBusy(true);
    setError(undefined);
    try {
      await action();
    } catch (reason) {
      setEdit(undefined);
      setError({ reason: reason instanceof Error ? reason.message : "The wallpaper could not be updated.", file });
    } finally {
      busyRef.current = false;
      setBusy(false);
    }
  }

  async function persist(next: ChatWallpaper | null, expected?: ChatWallpaper) {
    if (stage) {
      stage(next);
      return;
    }
    setEdit({ base: installed, value: next });
    await saveWallpaper(peerId, next, expected);
    project?.(next);
  }

  function chooseOwn(file: File | undefined) {
    if (!file) return;
    void run(async () => {
      const next = await uploadWallpaper(peerId, file);
      await persist(next);
      if (!stage && source?.asset && source.asset !== next.asset) await discardAsset(source.asset);
      setRequest(undefined);
    }, file.name);
  }

  async function discardAsset(id: string) {
    try {
      await deleteWallpaperAsset(peerId, id);
    } catch {
      setError({ reason: "The wallpaper changed. The previous file could not be removed." });
    }
  }

  const undo = request && installed?.asset && request.previous?.asset !== installed.asset
    && installed.asset_label === "General’s wallpaper.svg";
  const displayedTheme = light === themeIsLight ? theme : light ? lightTheme : DEFAULT_THEME;
  const mark = <span className="ct-wallpaper-tile__check" aria-hidden="true"><Check size={13} /></span>;
  const draft = (value: ChatWallpaper) => {
    if (stage) stage(value);
    else setEdit({ base: installed, value });
  };
  const change = (value: ChatWallpaper) => void run(() => persist(value));
  const restoreTheme = () => void run(async () => {
    await persist(null);
    setOwnOpen(false);
  });
  return <SettingsCard icon={<Wallpaper size={16} />} title={stage ? "Wallpaper" : "Chat wallpaper"} description={stage ? undefined : "Applies to chats on this machine"}
    className="ct-wallpaper-picker" headerRight={!stage && source && <button type="button" disabled={busy || !ready}
      className="ct-settings-link" onClick={restoreTheme}>Use theme wallpaper</button>}>
    {stage && installed && <div className="ct-appearance-try__wallpaper-overlay">
      <span>This machine’s own wallpaper stays on top.</span>
      <button type="button" disabled={busy || !ready} onClick={restoreTheme}>Use theme wallpaper</button>
    </div>}
    {!source && (active.background.pattern !== "none" || active.background.gradient) && <small>Theme wallpaper</small>}
    <div role="radiogroup" aria-label="Chat wallpaper pattern" className="ct-wallpaper-gallery">
      {WALLPAPER_PATTERNS.map(pattern => {
        const selected = !showOwn && active.background.pattern === pattern.id;
        const sample: ChatWallpaper = selected ? active : {
          background: { pattern: pattern.id, scale: pattern.id === "dots" ? 20 : 180, opacity: 0.24 },
          palette: "sample" in pattern && !themeIsLight ? pattern.sample : undefined,
          fill: "sample" in pattern && !themeIsLight ? { mode: "gradient", preset: "midnight" } : undefined,
        };
        return <label key={pattern.id} className="ct-wallpaper-tile" data-selected={selected || undefined}>
          <input type="radio" data-appearance-field="chat_wallpaper.background.pattern" name={group} className="sr-only" checked={selected} disabled={busy || !ready}
            onChange={() => { setOwnOpen(false); change({ background: { pattern: pattern.id } }); }} />
          <span className="ct-chat-wallpaper" aria-hidden="true"
            style={thumbnailStyle(theme, sample, themeIsLight, pattern.id === "dots" ? 20 : 180)} />
          {selected && mark}
          <span className="ct-wallpaper-tile__label">{pattern.label}</span>
        </label>;
      })}
      <label className={hasOwnFile ? "ct-wallpaper-tile" : "ct-wallpaper-tile ct-wallpaper-tile--own"}
        data-selected={showOwn || undefined}>
        <input type="radio" name={group} aria-label="Your own wallpaper" className="sr-only"
          checked={showOwn} disabled={!ready} onChange={() => setOwnOpen(true)} />
        {showOwn && mark}
        {hasOwnFile ? <>
          {asset && <span className="ct-chat-wallpaper" aria-hidden="true"
            style={thumbnailStyle(theme, active, themeIsLight, 80, asset.url)} />}
          <span className="ct-wallpaper-tile__label">Your own</span>
        </> : <>
          <Plus size={19} aria-hidden="true" />
          <span>Your own…</span>
          <small>SVG, image or ask an agent</small>
        </>}
      </label>
    </div>
    <div className="ct-wallpaper-editor">
      <div className="ct-wallpaper-edit">
        {showOwn && <>
          <input ref={pickerInput} className="sr-only" type="file" tabIndex={-1} aria-label="Upload wallpaper" data-appearance-field="chat_wallpaper.background.svg"
            accept={WALLPAPER_FILE_TYPES.join(",")} onChange={event => {
              chooseOwn(event.currentTarget.files?.[0]);
              event.currentTarget.value = "";
            }} />
          {hasOwnFile ? <div className="ct-wallpaper-own-file">
            <span className="ct-wallpaper-own-thumb" aria-hidden="true">
              {asset && <span className="ct-chat-wallpaper" style={thumbnailStyle(theme, active, themeIsLight, 28, asset.url)} />}
            </span>
            <div className="min-w-0 flex-1">
              <strong className="block truncate">{active.asset_label ?? "Uploaded wallpaper"}</strong>
              <small>{asset ? formatBytes(asset.size) + " · " : ""}saved for this machine{active.background.pattern === "svg" && " · lines follow the colour below"}</small>
            </div>
            <button type="button" className={UI_BUTTON.ghost} disabled={busy} onClick={() => pickerInput.current?.click()}>Replace</button>
            <button type="button" className={UI_BUTTON.ghost} disabled={busy} onClick={() => void run(async () => {
              await persist(null, active);
              if (!stage && active.asset) await discardAsset(active.asset);
              setRequest(undefined);
            })}>Remove</button>
          </div> : <div className="ct-wallpaper-drop" onDragOver={event => event.preventDefault()} onDrop={event => {
            event.preventDefault();
            chooseOwn(event.dataTransfer.files[0]);
          }}>
            <Upload size={22} aria-hidden="true" />
            <span>Drop an SVG or image, or <button type="button" disabled={busy} className="ct-wallpaper-link"
              onClick={() => pickerInput.current?.click()}>choose a file</button></span>
            <small>SVG up to 64 KB, drawn with strokes · PNG, JPEG or WebP up to 2 MB</small>
          </div>}
          {error && <div className="ct-wallpaper-error" role="alert">
            {error.file && <strong>{error.file} wasn’t used</strong>}
            <span>{error.reason}</span>
          </div>}
          {ask && <button type="button" className={UI_BUTTON.ghost} disabled={busy} onClick={ask}>Ask General for wallpaper</button>}
          {!stage && (undo ? <button type="button" className={UI_BUTTON.ghost} disabled={busy} onClick={() => void run(async () => {
            await persist(request.previous, installed);
            setRequest(undefined);
          })}>Undo</button> : <form className="ct-wallpaper-ask" onSubmit={event => {
            event.preventDefault();
            if (!description.trim()) return;
            void run(async () => {
              await askGeneralForWallpaper(peerId, description.trim());
              setRequest({ previous: installed });
              setDescription("");
            });
          }}>
            <label htmlFor={group + "-description"}>Ask an agent to draw one</label>
            <div>
              <input id={group + "-description"} className={UI_INPUT.full} value={description}
                placeholder="Cats and yarn, thin lines, loosely scattered" maxLength={500}
                onChange={event => setDescription(event.currentTarget.value)} />
              <button type="submit" disabled={busy || !description.trim()} className={UI_BUTTON.primary}>Ask General</button>
            </div>
            <small>{request ? "General is drawing your wallpaper." : "General draws a seamless tile for this machine and applies it here. You can undo it from this page."}</small>
          </form>)}
        </>}
        {(!showOwn || custom) && <WallpaperControls wallpaper={active} light={themeIsLight} disabled={!ready || busy}
          onDraft={draft} onChange={change} onReset={stage ? discard : () => change({ ...active, palette: undefined, fill: undefined, fill_strength: undefined,
            background: { pattern: active.background.pattern, icon: active.background.icon, svg: active.background.svg } })}
          resetLabel={stage ? "Discard wallpaper changes" : undefined} />}
        {showOwn && !custom && discard && <button type="button" className={UI_BUTTON.ghost} disabled={!ready} onClick={discard}>Discard wallpaper changes</button>}
        {error && !showOwn && <p className="ct-wallpaper-error" role="alert">{error.reason}</p>}
      </div>
      {!stage && <div className="ct-wallpaper-preview">
        <div className="ct-wallpaper-preview__head">
          <span>Preview</span>
          <div className="ct-wallpaper-segments" role="group" aria-label="Preview appearance">
            <button type="button" onClick={() => setLightOverride(true)} aria-pressed={light}>Light</button>
            <button type="button" onClick={() => setLightOverride(false)} aria-pressed={!light}>Dark</button>
          </div>
        </div>
        <div className="ct-wallpaper-preview__chat" data-preview-theme={light ? "light" : "dark"} style={{
          ...themeToCSS(displayedTheme),
          ...wallpaperRenderStyle(displayedTheme, source ?? undefined, light, asset?.url, machineColor),
        } as CSSProperties}>
          <span className="ct-chat-wallpaper" aria-hidden="true" />
          <div className="ct-wallpaper-preview__messages">
            <span data-user="true">Can you draw cats for my chat?</span>
            <span>Here’s a tile with line-art cats and yarn.</span>
            <span data-user="true">Perfect, keep it.</span>
          </div>
          <div className="ct-wallpaper-preview__composer">Message</div>
        </div>
      </div>}
    </div>
  </SettingsCard>;
}
