export function PermissionBypassStatus({ autoApprove }: { autoApprove: boolean }) {
  const bypassActive = autoApprove === true;
  const label = bypassActive
    ? "Provider permission bypass active for this session"
    : "Provider permissions and sandbox active for this session";

  return (
    <span
      role="status"
      aria-label={label}
      title={label}
      className={`absolute right-2 top-2 z-20 rounded border px-2 py-0.5 text-[10px] font-medium ${
        bypassActive
          ? "border-amber-500/60 bg-amber-950/80 text-amber-200"
          : "border-emerald-500/40 bg-emerald-950/70 text-emerald-200"
      }`}
    >
      {bypassActive ? "Permission bypass" : "Sandbox active"}
    </span>
  );
}
