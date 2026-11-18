import type { ProviderAccountRow } from "../../hooks/providerAccountsTypes";

const fontSizes = { 14: 8, 16: 9, 20: 11 };

export function AccountAvatar({ account, size, other, label }: {
  account: Pick<ProviderAccountRow, "name" | "identity">;
  size: 14 | 16 | 20;
  other: boolean;
  label?: string;
}) {
  const name = account.name?.trim() || account.identity?.trim() || "";
  const initial = Array.from(name.toUpperCase())[0] ?? "?";
  return <span role={label ? "img" : undefined} aria-hidden={label ? undefined : true} aria-label={label} title={label} className="inline-flex shrink-0 items-center justify-center self-center rounded-full font-semibold leading-none"
    style={{
      width: size,
      height: size,
      fontSize: fontSizes[size],
      color: other ? "var(--ct-accent)" : "var(--ct-muted-fg)",
      background: other ? "color-mix(in srgb, var(--ct-accent) 16%, transparent)" : "color-mix(in srgb, var(--ct-fg) 8%, transparent)",
    }}>{initial}</span>;
}
