import { useContext, useId, useRef, useState, type CSSProperties } from "react";
import { Check, Plus, Upload, Wallpaper } from "lucide-react";
import type { ChatWallpaper } from "../../../gen/ChatWallpaper";
import { formatBytes } from "../../lib/formatBytes";
import { isLightColor, DEFAULT_THEME, themeToCSS } from "../../lib/themeCss";
import { MachineAppearanceContext, useWallpaperAsset } from "../../lib/useMachineAppearance";
import { effectiveWallpaper, WALLPAPER_PATTERNS, wallpaperRenderStyle } from "../../lib/chatWallpaper";
import { askGeneralForWallpaper, deleteWallpaperAsset, saveWallpaper, uploadWallpaper, WALLPAPER_FILE_TYPES } from "../../lib/chatWallpaperClient";
import type { AppConfig, ThemeConfig } from "../../lib/types";
import { UI_BUTTON, UI_INPUT } from "../shared/design";
import { SettingsCard } from "./shared";
import { WallpaperControls } from "./WallpaperControls";

const lightTheme: ThemeConfig = {
  ...DEFAULT_THEME, name: "github-light", background: "#f6f8fa", foreground: "#1f2328", blue: "#0969da",
};

export function ChatWallpaperPicker({ config, showOwnInitially }: { config: AppConfig; showOwnInitially?: boolean }) {
  const selected = useContext(MachineAppearanceContext);
  const appearance = selected?.appearance ?? { theme: config.theme, wallpaper: config.chat_wallpaper ?? null };
  return <Picker key={selected?.peerId ?? "local"} theme={appearance.theme} installed={appearance.wallpaper}
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
}

function Picker({ theme, installed, machineColor, peerId, ready, project, showOwnInitially }: PickerProps) {
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
  const showOwn = ownOpen || custom;
  const asset = useWallpaperAsset(peerId, ready ? active.asset : undefined);
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
    setEdit({ base: installed, value: next });
    await saveWallpaper(peerId, next, expected);
    project?.(next);
  }

  function chooseOwn(file: File | undefined) {
    if (!file) return;
    void run(async () => {
      const next = await uploadWallpaper(peerId, file);
      await persist(next);
      if (source?.asset && source.asset !== next.asset) await discardAsset(source.asset);
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
  const draft = (value: ChatWallpaper) => setEdit({ base: installed, value });
  const change = (value: ChatWallpaper) => void run(() => persist(value));
  const restoreTheme = () => void run(async () => {
    await persist(null);
    setOwnOpen(false);
  });
  return <SettingsCard icon={<Wallpaper size={16} />} title="Chat wallpaper" description="Applies to chats on this machine"
    className="ct-wallpaper-picker" headerRight={source && <button type="button" disabled={busy || !ready}
      className="ct-settings-link" onClick={restoreTheme}>Use theme wallpaper</button>}>
    <div role="radiogroup" aria-label="Chat wallpaper pattern" className="ct-wallpaper-gallery">
      {WALLPAPER_PATTERNS.map(pattern => {
        const selected = !showOwn && active.background.pattern === pattern.id;
        const sample: ChatWallpaper = selected ? active : {
          background: { pattern: pattern.id, scale: pattern.id === "dots" ? 20 : 180, opacity: 0.24 },
          palette: "sample" in pattern && !themeIsLight ? pattern.sample : undefined,
          fill: "sample" in pattern && !themeIsLight ? { mode: "gradient", preset: "midnight" } : undefined,
        };
        const thumbnail = { ...sample, background: { ...sample.background, scale: pattern.id === "dots" ? 20 : 180 } };
        return <label key={pattern.id} className="ct-wallpaper-tile" data-selected={selected || undefined}>
          <input type="radio" name={group} className="sr-only" checked={selected} disabled={busy || !ready}
            onChange={() => { setOwnOpen(false); change({ background: { pattern: pattern.id } }); }} />
          <span className="ct-chat-wallpaper" aria-hidden="true"
            style={wallpaperRenderStyle(theme, thumbnail, themeIsLight) as CSSProperties} />
          {selected && mark}
          <span className="ct-wallpaper-tile__label">{pattern.label}</span>
        </label>;
      })}
      <label className="ct-wallpaper-tile ct-wallpaper-tile--own" data-selected={showOwn || undefined}>
        <input type="radio" name={group} aria-label="Your own wallpaper" className="sr-only"
          checked={showOwn} disabled={!ready} onChange={() => setOwnOpen(true)} />
        {showOwn && mark}
        <Plus size={19} aria-hidden="true" />
        <span>Your own…</span>
        <small>SVG, image or ask an agent</small>
      </label>
    </div>
    <div className="ct-wallpaper-editor">
      <div className="ct-wallpaper-edit">
        {showOwn && <>
          <input ref={pickerInput} className="sr-only" type="file" tabIndex={-1} aria-label="Upload wallpaper"
            accept={WALLPAPER_FILE_TYPES.join(",")} onChange={event => {
              chooseOwn(event.currentTarget.files?.[0]);
              event.currentTarget.value = "";
            }} />
          {custom && active.asset ? <div className="ct-wallpaper-own-file">
            <span className="ct-wallpaper-own-thumb" aria-hidden="true"><span className="ct-chat-wallpaper"
              style={wallpaperRenderStyle(theme, active, themeIsLight, asset?.url) as CSSProperties} /></span>
            <div className="min-w-0 flex-1">
              <strong className="block truncate">{active.asset_label ?? "Uploaded wallpaper"}</strong>
              <small>{asset ? formatBytes(asset.size) + " · " : ""}saved for this machine{active.background.pattern === "svg" && " · lines follow the colour below"}</small>
            </div>
            <button type="button" className={UI_BUTTON.ghost} disabled={busy} onClick={() => pickerInput.current?.click()}>Replace</button>
            <button type="button" className={UI_BUTTON.ghost} disabled={busy} onClick={() => void run(async () => {
              await persist(null, active);
              if (active.asset) await discardAsset(active.asset);
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
          {undo ? <button type="button" className={UI_BUTTON.ghost} disabled={busy} onClick={() => void run(async () => {
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
          </form>}
        </>}
        {(!showOwn || custom) && <WallpaperControls wallpaper={active} light={themeIsLight} disabled={!ready || busy}
          onDraft={draft} onChange={change} onReset={() => change({ ...active, palette: undefined, fill: undefined,
            background: { pattern: active.background.pattern, icon: active.background.icon, svg: active.background.svg } })} />}
        {error && !showOwn && <p className="ct-wallpaper-error" role="alert">{error.reason}</p>}
      </div>
      <div className="ct-wallpaper-preview">
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
      </div>
    </div>
  </SettingsCard>;
}
