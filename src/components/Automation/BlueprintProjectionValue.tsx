import { useState } from "react";

export function BlueprintProjectionValue({ value, label }: { value: unknown; label: string }) {
  const [open, setOpen] = useState(false);
  if (value === null || typeof value !== "object") return <pre aria-label={label} className="whitespace-pre-wrap break-words">{value === undefined ? "unavailable" : typeof value === "string" ? value : String(value)}</pre>;
  return <details aria-label={label} open={open} onToggle={(event) => setOpen(event.currentTarget.open)}>
    <summary>{label}</summary>
    {open && <dl>{Object.entries(value).map(([key, child]) => <div key={key}><dt>{key}</dt><dd><BlueprintProjectionValue value={child} label={`${label}.${key}`} /></dd></div>)}</dl>}
  </details>;
}
