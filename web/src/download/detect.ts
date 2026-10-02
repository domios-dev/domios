import { installers } from './catalog';
import type { Installer } from './catalog';

export type ClientHints = {
  platform: string;
  mobile?: boolean;
  getHighEntropyValues?: (hints: string[]) => Promise<{ architecture?: string; bitness?: string }>;
};

export function detectInstaller(userAgent: string, platform = '', architecture = '', mobile = false): Installer | null {
  if (mobile || /Android|iPhone|iPad|iPod|Mobile|CrOS/i.test(userAgent)) return null;
  const os = /win/i.test(platform) || /Windows/i.test(userAgent) ? 'windows'
    : /mac/i.test(platform) || /Macintosh|Mac OS X/i.test(userAgent) ? 'mac'
    : /linux/i.test(platform) || /Linux|X11/i.test(userAgent) ? 'linux' : null;
  if (!os) return null;
  const arm = /arm|aarch64/i.test(architecture || userAgent);
  const intelMac = os === 'mac' && /x86|x64/i.test(architecture);
  const arch = os === 'mac' ? intelMac ? 'x64' : 'arm64' : arm ? 'arm64' : 'x64';
  return installers.find(value => value.os === os && value.arch === arch) ?? null;
}
