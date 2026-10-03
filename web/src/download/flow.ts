import { installers, isInstallerUrl } from './catalog';
import type { DownloadInfo, Installer } from './catalog';

export type DownloadAsset = DownloadInfo['downloads'][number] & { asset: string };
export type DownloadFeedbackState =
  | { status: 'idle' | 'unsupported'; installer: null }
  | { status: 'pending'; installer: Installer }
  | { status: 'error'; installer: Installer; message: string }
  | { status: 'starting'; installer: Installer; asset: DownloadAsset; info: DownloadInfo };

const idle: DownloadFeedbackState = { status: 'idle', installer: null };

function record(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null;
}

export function isDirectAssetUrl(value: string) {
  if (!isInstallerUrl(value)) return false;
  try {
    const url = new URL(value);
    return url.protocol === 'https:' && url.hostname === 'github.com' && !url.port
      && !url.username && !url.password && !url.search && !url.hash
      && /^\/rollacode\/codeterm-releases\/releases\/download\/[^/]+\/[^/]+$/.test(url.pathname);
  } catch { return false; }
}

function assetName(href: string) {
  try { return decodeURIComponent(new URL(href).pathname.split('/').at(-1)!); }
  catch { return null; }
}

function matchesSelection(asset: string, installer: Installer) {
  return /^CodeTerm_[0-9]+\.[0-9]+\.[0-9]+(?:-[A-Za-z0-9.-]+)?_/.test(asset) && asset.endsWith(`_${installer.suffix}`);
}

export function parseDownloadInfo(value: unknown): DownloadInfo {
  if (!record(value) || (value.version !== null && typeof value.version !== 'string')
    || !Array.isArray(value.downloads)) throw new Error('The release details could not be read. Please try again.');
  const ids = new Set<string>();
  const downloads = value.downloads.map((entry: unknown) => {
    if (!record(entry) || typeof entry.id !== 'string' || ids.has(entry.id)
      || typeof entry.href !== 'string' || typeof entry.available !== 'boolean'
      || (entry.asset !== null && typeof entry.asset !== 'string')) {
      throw new Error('The release details could not be read. Please try again.');
    }
    ids.add(entry.id);
    const installer = installers.find(installer => installer.id === entry.id);
    if (entry.available && (!isDirectAssetUrl(entry.href) || !entry.asset || !installer
      || assetName(entry.href) !== entry.asset || !matchesSelection(entry.asset, installer))) {
      throw new Error('The installer link could not be verified. Please try again or browse releases.');
    }
    return { id: entry.id, href: entry.href, available: entry.available, asset: entry.asset };
  });
  return { version: value.version, downloads };
}

export function selectedAsset(info: DownloadInfo, installer: Installer): DownloadAsset {
  const asset = info.downloads.find(entry => entry.id === installer.id && entry.available);
  if (!asset || !asset.asset || !isDirectAssetUrl(asset.href) || !matchesSelection(asset.asset, installer)) {
    throw new Error('This installer is currently unavailable. Choose another platform or browse releases.');
  }
  return { ...asset, asset: asset.asset };
}

export async function lookupDownloadInfo(signal: AbortSignal, fetcher: typeof fetch = fetch,
  deadline: AbortSignal = AbortSignal.timeout(7000)): Promise<DownloadInfo> {
  const bounded = AbortSignal.any([signal, deadline]);
  let rejectAbort: (reason: unknown) => void;
  const fail = () => rejectAbort(bounded.reason);
  const aborted = new Promise<never>((_, reject) => {
    rejectAbort = reject;
    if (bounded.aborted) reject(bounded.reason);
    else bounded.addEventListener('abort', fail, { once: true });
  });
  try {
    return await Promise.race([aborted, (async () => {
      bounded.throwIfAborted();
      const response = await fetcher('/api/download/info', { signal: bounded, cache: 'no-cache' });
      if (!response.ok) throw new Error('Release details are unavailable right now. Please try again.');
      return parseDownloadInfo(await response.json());
    })()]);
  } catch (error) {
    if (deadline.aborted) throw new Error('The release lookup timed out. Please try again.');
    if (signal.aborted) throw error;
    if (error instanceof Error && !(error instanceof TypeError) && !(error instanceof SyntaxError)) throw error;
    throw new Error('Release details could not be reached or read. Please try again.');
  } finally { bounded.removeEventListener('abort', fail); }
}

export function createDownloadFlow({ lookup = lookupDownloadInfo, initiate, change }: {
  lookup?: (signal: AbortSignal) => Promise<DownloadInfo>;
  initiate: (asset: DownloadAsset) => void;
  change: (state: DownloadFeedbackState) => void;
}) {
  let state: DownloadFeedbackState = idle;
  let generation = 0;
  let request: AbortController | undefined;
  const publish = (next: DownloadFeedbackState) => { state = next; change(next); };
  const close = () => {
    generation++;
    request?.abort();
    request = undefined;
    publish(idle);
  };
  const start = async (installer: Installer | null) => {
    if ((state.status === 'pending' || state.status === 'starting') && state.installer.id === installer?.id) return;
    const current = ++generation;
    request?.abort();
    request = undefined;
    if (!installer) { publish({ status: 'unsupported', installer: null }); return; }
    const controller = new AbortController();
    request = controller;
    publish({ status: 'pending', installer });
    try {
      const info = parseDownloadInfo(await lookup(controller.signal));
      if (current !== generation || controller.signal.aborted) return;
      const asset = selectedAsset(info, installer);
      initiate(asset);
      publish({ status: 'starting', installer, asset, info });
    } catch (error) {
      if (current !== generation || controller.signal.aborted) return;
      publish({ status: 'error', installer, message: error instanceof Error ? error.message : 'The download could not be started. Please try again.' });
    } finally {
      if (current === generation) request = undefined;
    }
  };
  return { start, close };
}

export function ordinaryDownloadClick(event: {
  button: number; metaKey: boolean; ctrlKey: boolean; shiftKey: boolean; altKey: boolean; defaultPrevented: boolean;
}) {
  return !event.defaultPrevented && event.button === 0 && !event.metaKey && !event.ctrlKey && !event.shiftKey && !event.altKey;
}
