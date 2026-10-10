export interface NodeFile {
  name: string;
  path: string;
  short_path: string;
  exists: boolean;
}

export function revealFileLabel(platform: string) {
  if (/mac/i.test(platform)) return "Show in Finder";
  if (/win/i.test(platform)) return "Show in Explorer";
  return "Show in File Manager";
}

export function filePreviewKind(path: string): "document" | "image" | "page" | null {
  const extension = path.split(/[\\/]/).pop()?.split(".").pop()?.toLowerCase();
  if (extension && ["md", "markdown", "txt", "log", "json", "yaml", "yml", "toml", "csv"].includes(extension)) return "document";
  if (extension && ["png", "jpg", "jpeg", "gif", "webp", "bmp", "avif", "svg"].includes(extension)) return "image";
  if (extension && ["html", "htm"].includes(extension)) return "page";
  return null;
}
