import type { RecordedFileChange } from "@codeterm/chat-core";

export function changesTotals(files: readonly RecordedFileChange[]) {
  return files.reduce((total, file) => ({ additions: total.additions + file.additions, deletions: total.deletions + file.deletions }), { additions: 0, deletions: 0 });
}

export function changesDirectories(files: readonly RecordedFileChange[]) {
  const directories = new Map<string, RecordedFileChange[]>();
  for (const file of files) {
    const path = (file.movePath ?? file.path).replace(/\\/g, "/");
    const directory = path.includes("/") ? path.slice(0, path.lastIndexOf("/")) : ".";
    const siblings = directories.get(directory) ?? [];
    siblings.push(file);
    directories.set(directory, siblings);
  }
  return Array.from(directories).sort(([a], [b]) => a.localeCompare(b)).map(([path, children]) => ({
    path, files: children.sort((a, b) => (a.movePath ?? a.path).localeCompare(b.movePath ?? b.path)), ...changesTotals(children),
  }));
}

export function changesFilePath(file: RecordedFileChange, cwd?: string | null): string | null {
  const path = file.movePath ?? file.path;
  if (/^(?:[A-Za-z]:[\\/]|[\\/]|~[\\/])/.test(path)) return path;
  return cwd ? `${cwd.replace(/[\\/]$/, "")}/${path}` : null;
}
