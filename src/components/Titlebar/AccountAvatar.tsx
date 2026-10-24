import { useState } from "react";

const SIZES = {
  tray: "h-5 w-5 text-[11px]",
  menu: "h-[30px] w-[30px] shrink-0 text-[12px]",
} as const;

export function AccountAvatar({ url, initial, label, size }: {
  url: string | null;
  initial: string;
  label: string;
  size: keyof typeof SIZES;
}) {
  const [failedUrl, setFailedUrl] = useState<string | null>(null);
  const showImage = url !== null && url !== failedUrl;
  return <span className={`relative flex shrink-0 items-center justify-center overflow-hidden rounded-full bg-[color-mix(in_srgb,var(--ct-accent)_22%,transparent)] font-semibold text-[var(--ct-accent)] ${SIZES[size]}`}>
    <span aria-hidden="true">{initial}</span>
    {showImage && <img src={url} alt={label} referrerPolicy="no-referrer" decoding="async" draggable={false}
      className="absolute inset-0 h-full w-full object-cover" onError={() => setFailedUrl(url)} />}
  </span>;
}
