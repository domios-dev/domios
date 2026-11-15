export function ChecklistReadFailure({ onRetry }: { onRetry: () => void }) {
  return <span className="sidebar-agent-checklist__note">
    <span role="status">Could not read the saved checklist.</span>{" "}
    <button type="button" className="underline" onClick={(event) => { event.stopPropagation(); onRetry(); }}>Retry checklist</button>
  </span>;
}
