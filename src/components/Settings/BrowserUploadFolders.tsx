import { useState } from "react";
import { FolderOpen } from "lucide-react";
import type { AppConfig } from "../../lib/types";
import { SettingsCard } from "./shared";

export function BrowserUploadFolders({ config, onSave }: {
  config: AppConfig;
  onSave: (config: AppConfig) => Promise<void>;
}) {
  const [profile, setProfile] = useState("");
  const [folder, setFolder] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const grants = config.browser_upload_folders ?? {};
  async function save(next: Record<string, string[]>) {
    setBusy(true);
    setError("");
    try {
      await onSave({ ...config, browser_upload_folders: next });
      setFolder("");
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setBusy(false);
    }
  }
  return <SettingsCard icon={<FolderOpen size={16} />} title="Browser upload folders"
    description="Allow agents using a named Browser profile to upload files from these folders and their subfolders. Only the owner can change this list.">
    <ul aria-label="Granted browser upload folders">
      {Object.entries(grants).flatMap(([name, folders]) => folders.map(path =>
        <li key={JSON.stringify([name, path])} className="flex items-center gap-2 py-1">
          <span className="flex-1 break-all">{name}: {path}</span>
          <button type="button" className="ct-bar-pill" disabled={busy}
            aria-label={'Remove ' + path + ' from ' + name}
            onClick={() => {
              const next = { ...grants, [name]: folders.filter(value => value !== path) };
              if (!next[name].length) delete next[name];
              void save(next);
            }}>Remove</button>
        </li>))}
    </ul>
    <label className="ct-settings-toggle">Browser profile
      <input className="ct-field" aria-label="Browser profile" value={profile} disabled={busy}
        onInput={event => setProfile(event.currentTarget.value)} placeholder="work" />
    </label>
    <label className="ct-settings-toggle">Absolute folder
      <input className="ct-field" aria-label="Absolute upload folder" value={folder} disabled={busy}
        onInput={event => setFolder(event.currentTarget.value)} placeholder="C:/Developer/media" />
    </label>
    <button type="button" className="ct-bar-pill" disabled={busy || !profile || !folder}
      onClick={() => void save({ ...grants, [profile]: [...new Set([...(grants[profile] ?? []), folder])] })}>
      Grant folder
    </button>
    {error && <p role="alert">{error}</p>}
  </SettingsCard>;
}
