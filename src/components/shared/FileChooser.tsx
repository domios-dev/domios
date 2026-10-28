import { useRef, useState } from "react";
import { FileUp } from "lucide-react";
import { Button } from "./Button";

/** A file field in the app's own controls: a button opens the system chooser and the chosen file's name reads back beside it.
 *  The native input stays hidden, so its browser-locale caption never shows. */
export function FileChooser({ id, accept, disabled, label = "Choose file…", onFile }: {
  id?: string; accept?: string; disabled?: boolean; label?: string; onFile: (file: File | undefined) => void;
}) {
  const input = useRef<HTMLInputElement>(null);
  const [name, setName] = useState<string | null>(null);
  return <div className="flex min-w-0 items-center gap-2">
    <input ref={input} id={id} type="file" accept={accept} disabled={disabled} className="hidden"
      onChange={(event) => { const file = event.currentTarget.files?.[0]; setName(file?.name ?? null); onFile(file); }} />
    <Button size="sm" disabled={disabled} onClick={() => input.current?.click()}><FileUp size={12} aria-hidden="true" />{label}</Button>
    <span className="min-w-0 truncate text-xs text-[var(--ct-muted-fg)]" title={name ?? undefined}>{name ?? "No file chosen"}</span>
  </div>;
}
