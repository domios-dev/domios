export function ActiveTabPill() {
  return (
    <div
      className="active-pane-pill absolute inset-y-0 left-0 right-0 pointer-events-none"
      style={{
        background: "var(--ct-bg-selected, rgba(99, 102, 241, 0.12))",
        transition: "transform 320ms cubic-bezier(0.16, 1, 0.3, 1), opacity 200ms ease",
      }}
    />
  );
}
