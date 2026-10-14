export function ChatDaySeparator({ label }: { label?: string }) {
  if (!label) return null;
  return <div role="separator" aria-label={label} className="mx-auto mb-3 flex max-w-4xl items-center gap-3 text-[11px]" style={{ color: "var(--ct-muted-fg)" }}>
    <span className="h-px flex-1" style={{ background: "var(--ct-border)" }} />
    <span>{label}</span>
    <span className="h-px flex-1" style={{ background: "var(--ct-border)" }} />
  </div>;
}
