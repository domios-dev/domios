export const releasesPage = 'https://github.com/rollacode/codeterm-releases/releases/latest';

export const installers = [
  { id: 'mac-arm64-dmg', os: 'mac', arch: 'arm64', format: 'dmg', platform: 'macOS', detail: 'Apple Silicon', label: 'macOS · Apple Silicon', suffix: 'aarch64.dmg' },
  { id: 'mac-x64-dmg', os: 'mac', arch: 'x64', format: 'dmg', platform: 'macOS', detail: 'Intel Mac', label: 'macOS · Intel Mac', suffix: 'x64.dmg' },
  { id: 'windows-x64-exe', os: 'windows', arch: 'x64', format: 'exe', platform: 'Windows', detail: 'x64', label: 'Windows · x64', suffix: 'x64-setup.exe' },
  { id: 'windows-arm64-exe', os: 'windows', arch: 'arm64', format: 'exe', platform: 'Windows', detail: 'ARM64', label: 'Windows · ARM64', suffix: 'arm64-setup.exe' },
  { id: 'linux-x64-deb', os: 'linux', arch: 'x64', format: 'deb', platform: 'Linux', detail: 'x64 · .deb', label: 'Linux · x64 · .deb', suffix: 'amd64.deb' },
  { id: 'linux-arm64-deb', os: 'linux', arch: 'arm64', format: 'deb', platform: 'Linux', detail: 'ARM64 · .deb', label: 'Linux · ARM64 · .deb', suffix: 'arm64.deb' },
  { id: 'linux-x64-appimage', os: 'linux', arch: 'x64', format: 'appimage', platform: 'Linux', detail: 'x64 · AppImage', label: 'Linux · x64 · AppImage', suffix: 'amd64.AppImage' },
  { id: 'linux-arm64-appimage', os: 'linux', arch: 'arm64', format: 'appimage', platform: 'Linux', detail: 'ARM64 · AppImage', label: 'Linux · ARM64 · AppImage', suffix: 'aarch64.AppImage' },
] as const;

export type Installer = typeof installers[number];
export type DownloadInfo = {
  version: string | null;
  downloads: { id: string; href: string; available: boolean; asset: string | null }[];
};

export function downloadHref(installer: Installer) {
  return `/api/download?os=${installer.os}&arch=${installer.arch}&format=${installer.format}`;
}

export function isInstallerUrl(value: string) {
  return value.startsWith('https://github.com/rollacode/codeterm-releases/releases/download/');
}
