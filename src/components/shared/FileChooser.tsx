import { useRef, useState, type DragEvent } from "react";
import { FileUp, X } from "lucide-react";
import { formatBytes } from "../../lib/formatBytes";

/** A file field in the app's own controls: a drop area that also opens the system chooser, then the chosen file's name and
 *  size with a way to clear it. The native input stays hidden, so its browser-locale caption never shows. */
export function FileChooser({ id, accept, disabled, prompt = "Drop a file or choose one", onFile }: {
  id?: string; accept?: string; disabled?: boolean; prompt?: string; onFile: (file: File | undefined) => void;
}) {
  const input = useRef<HTMLInputElement>(null);
  const [file, setFile] = useState<{ name: string; size: number } | null>(null);
  const [over, setOver] = useState(false);
  const take = (next: File | undefined) => { setFile(next ? { name: next.name, size: next.size } : null); onFile(next); };
  const clear = () => { if (input.current) input.current.value = ""; take(undefined); };
  const drop = (event: DragEvent<HTMLElement>) => {
    event.preventDefault();
    setOver(false);
    if (!disabled) take(event.dataTransfer.files?.[0]);
  };
  return <div className="ct-file-chooser" data-over={over || undefined} data-chosen={file ? true : undefined} aria-disabled={disabled || undefined}
    onDragOver={(event) => { event.preventDefault(); if (!disabled) setOver(true); }} onDragLeave={() => setOver(false)} onDrop={drop}>
    <input ref={input} id={id} type="file" accept={accept} disabled={disabled} className="hidden"
      onChange={(event) => take(event.currentTarget.files?.[0])} />
    <FileUp size={16} aria-hidden="true" className="ct-file-chooser__icon" />
    {file
      ? <span className="ct-file-chooser__file">
        <span className="ct-file-chooser__name" title={file.name}>{file.name}</span>
        <span className="ct-file-chooser__meta">{formatBytes(file.size)}</span>
      </span>
      : <button type="button" className="ct-file-chooser__prompt" disabled={disabled} onClick={() => input.current?.click()}>{prompt}</button>}
    {file && <button type="button" className="ct-file-chooser__remove" aria-label={`Remove ${file.name}`} disabled={disabled} onClick={clear}>
      <X size={13} aria-hidden="true" /></button>}
  </div>;
}
