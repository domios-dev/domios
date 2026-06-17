import React from "react";

import extensionsSvg from "./svg/extensions.svg?raw";
import pinSvg from "./svg/pin.svg?raw";

// ── Custom icons (not in Lucide) ─────────────────────────────────────

interface IconProps {
  size?: number;
  className?: string;
  style?: React.CSSProperties;
  title?: string;
}

/** Render a raw .svg file at a fixed pixel size; color inherits via currentColor.
 * `fill` swaps the source's `fill="none"` for a filled rendering. */
function RawIcon({
  raw,
  size,
  className,
  style,
  fill,
}: { raw: string; size: number; fill?: boolean } & Omit<IconProps, "size">) {
  let svg = raw.replace(/<svg/, `<svg width="${size}" height="${size}"`);
  if (fill) svg = svg.replace('fill="none"', 'fill="currentColor"');
  return (
    <span
      className={className}
      style={{ display: "inline-flex", lineHeight: 0, ...style }}
      dangerouslySetInnerHTML={{ __html: svg }}
    />
  );
}

/** Telegram logo */
export function TelegramIcon({ size = 10, className, style }: IconProps) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="currentColor" className={className} style={style}>
      <path d="M11.944 0A12 12 0 0 0 0 12a12 12 0 0 0 12 12 12 12 0 0 0 12-12A12 12 0 0 0 12 0a12 12 0 0 0-.056 0zm4.962 7.224c.1-.002.321.023.465.14a.506.506 0 0 1 .171.325c.016.093.036.306.02.472-.18 1.898-.962 6.502-1.36 8.627-.168.9-.499 1.201-.82 1.23-.696.065-1.225-.46-1.9-.902-1.056-.693-1.653-1.124-2.678-1.8-1.185-.78-.417-1.21.258-1.91.177-.184 3.247-2.977 3.307-3.23.007-.032.014-.15-.056-.212s-.174-.041-.249-.024c-.106.024-1.793 1.14-5.061 3.345-.48.33-.913.49-1.302.48-.428-.008-1.252-.241-1.865-.44-.752-.245-1.349-.374-1.297-.789.027-.216.325-.437.893-.663 3.498-1.524 5.83-2.529 6.998-3.014 3.332-1.386 4.025-1.627 4.476-1.635z" />
    </svg>
  );
}

/** GitHub Octicon terminal — used for tab/pane icons */
export function TerminalIcon({ size = 12, className, style }: IconProps) {
  return (
    <svg width={size} height={size} viewBox="0 0 16 16" fill="currentColor" className={className} style={style}>
      <path d="M0 2.75C0 1.784.784 1 1.75 1h12.5c.966 0 1.75.784 1.75 1.75v10.5A1.75 1.75 0 0 1 14.25 15H1.75A1.75 1.75 0 0 1 0 13.25Zm1.75-.25a.25.25 0 0 0-.25.25v10.5c0 .138.112.25.25.25h12.5a.25.25 0 0 0 .25-.25V2.75a.25.25 0 0 0-.25-.25ZM3.5 4.5l3 2.5-3 2.5V4.5ZM8.5 10h3v1.5h-3V10Z" />
    </svg>
  );
}

/** Octicon close X (filled style, 10x10) — for close buttons */
export function CloseXIcon({ size = 10, className, style }: IconProps) {
  return (
    <svg width={size} height={size} viewBox="0 0 10 10" fill="currentColor" className={className} style={style}>
      <path d="M1.41 0L5 3.59 8.59 0 10 1.41 6.41 5 10 8.59 8.59 10 5 6.41 1.41 10 0 8.59 3.59 5 0 1.41Z" />
    </svg>
  );
}

/** Octicon folder closed */
export function FolderIcon({ size = 14, className, style, open }: IconProps & { open?: boolean }) {
  return (
    <svg width={size} height={size} viewBox="0 0 16 16" fill="currentColor" className={className} style={style}>
      {open ? (
        <path d="M.513 1.513A1.75 1.75 0 0 1 1.75 1h3.5c.55 0 1.07.26 1.4.7l.9 1.2a.25.25 0 0 0 .2.1h6.5A1.75 1.75 0 0 1 16 4.75v.462a.25.25 0 0 1-.03.118l-2.39 4.608a1.75 1.75 0 0 1-1.553.937H1.75A1.75 1.75 0 0 1 0 9.125V2.75c0-.464.184-.91.513-1.237Z" />
      ) : (
        <path d="M1.75 1A1.75 1.75 0 0 0 0 2.75v10.5C0 14.216.784 15 1.75 15h12.5A1.75 1.75 0 0 0 16 13.25v-8.5A1.75 1.75 0 0 0 14.25 3H7.5a.25.25 0 0 1-.2-.1l-.9-1.2C6.07 1.26 5.55 1 5 1H1.75Z" />
      )}
    </svg>
  );
}

/** Octicon gear — settings */
export function GearIcon({ size = 14, className, style }: IconProps) {
  return (
    <svg width={size} height={size} viewBox="0 0 16 16" fill="currentColor" className={className} style={style}>
      <path d="M8 0a8.2 8.2 0 0 1 .701.031C9.444.095 9.99.645 10.16 1.29l.288 1.107c.018.066.079.158.212.224.231.114.454.243.668.386.123.082.233.09.3.071l1.102-.303c.644-.176 1.392.021 1.82.63.27.385.506.792.704 1.218.315.675.111 1.422-.364 1.891l-.814.806c-.049.048-.098.147-.088.294a6.57 6.57 0 0 1 0 .772c-.01.147.04.246.088.294l.814.806c.475.469.679 1.216.364 1.891a7.977 7.977 0 0 1-.704 1.217c-.428.61-1.176.807-1.82.63l-1.103-.303c-.066-.019-.176-.011-.299.071a5.909 5.909 0 0 1-.668.386c-.133.066-.194.158-.212.224l-.288 1.107c-.17.645-.716 1.195-1.459 1.26a8.006 8.006 0 0 1-1.402 0c-.743-.065-1.289-.615-1.459-1.26l-.289-1.107c-.017-.066-.078-.158-.211-.224a5.909 5.909 0 0 1-.668-.386c-.123-.082-.233-.09-.3-.071l-1.102.303c-.644.176-1.392-.021-1.82-.63a8.012 8.012 0 0 1-.704-1.218c-.315-.675-.111-1.422.363-1.891l.815-.806c.049-.048.098-.147.088-.294a6.57 6.57 0 0 1 0-.772c.01-.147-.04-.246-.088-.294l-.815-.806C.635 6.045.431 5.298.746 4.623a7.92 7.92 0 0 1 .704-1.217c.428-.61 1.176-.807 1.82-.63l1.103.303c.066.019.176.011.299-.071.214-.143.437-.272.668-.386.133-.066.194-.158.212-.224L5.84 1.29c.17-.645.716-1.195 1.459-1.26A8.2 8.2 0 0 1 8 0ZM5.5 8a2.5 2.5 0 1 0 5 0 2.5 2.5 0 0 0-5 0Z" />
    </svg>
  );
}

/** Octicon hamburger menu */
export function HamburgerIcon({ size = 15, className, style }: IconProps) {
  return (
    <svg width={size} height={size} viewBox="0 0 16 16" fill="currentColor" className={className} style={style}>
      <path d="M1 2.75A.75.75 0 0 1 1.75 2h12.5a.75.75 0 0 1 0 1.5H1.75A.75.75 0 0 1 1 2.75Zm0 5A.75.75 0 0 1 1.75 7h12.5a.75.75 0 0 1 0 1.5H1.75A.75.75 0 0 1 1 7.75ZM1.75 12h12.5a.75.75 0 0 1 0 1.5H1.75a.75.75 0 0 1 0-1.5Z" />
    </svg>
  );
}

/** Octicon send (paper plane) */
export function SendIcon({ size = 14, className, style }: IconProps) {
  return (
    <svg width={size} height={size} viewBox="0 0 16 16" fill="currentColor" className={className} style={style}>
      <path d="M.989 8 .064 2.68a1.342 1.342 0 011.85-1.462l13.402 5.744a1.13 1.13 0 010 2.076L1.913 14.782a1.343 1.343 0 01-1.85-1.463L.99 8Zm1.015.78l-.616 3.559a.14.14 0 00.193.153l10.24-4.387-9.817.674Zm-.616-5.119.616 3.559 9.817.674-10.24-4.387a.14.14 0 00-.193.154Z" />
    </svg>
  );
}

/** Octicon copy/clipboard */
export function CopyIcon({ size = 10, className, style }: IconProps) {
  return (
    <svg width={size} height={size} viewBox="0 0 16 16" fill="currentColor" className={className} style={style}>
      <path d="M0 6.75C0 5.784.784 5 1.75 5h1.5a.75.75 0 0 1 0 1.5h-1.5a.25.25 0 0 0-.25.25v7.5c0 .138.112.25.25.25h7.5a.25.25 0 0 0 .25-.25v-1.5a.75.75 0 0 1 1.5 0v1.5A1.75 1.75 0 0 1 9.25 16h-7.5A1.75 1.75 0 0 1 0 14.25ZM5 1.75C5 .784 5.784 0 6.75 0h7.5C15.216 0 16 .784 16 1.75v7.5A1.75 1.75 0 0 1 14.25 11h-7.5A1.75 1.75 0 0 1 5 9.25Zm1.75-.25a.25.25 0 0 0-.25.25v7.5c0 .138.112.25.25.25h7.5a.25.25 0 0 0 .25-.25v-7.5a.25.25 0 0 0-.25-.25Z" />
    </svg>
  );
}

/** Octicon checkmark */
export function CheckIcon({ size = 10, className, style }: IconProps) {
  return (
    <svg width={size} height={size} viewBox="0 0 16 16" fill="currentColor" className={className} style={style}>
      <path d="M13.78 4.22a.75.75 0 0 1 0 1.06l-7.25 7.25a.75.75 0 0 1-1.06 0L2.22 9.28a.75.75 0 0 1 1.06-1.06L6 10.94l6.72-6.72a.75.75 0 0 1 1.06 0Z" />
    </svg>
  );
}

/** Octicon chevron-up (16x16, filled) */
export function ChevronUpIcon({ size = 10, className, style }: IconProps) {
  return (
    <svg width={size} height={size} viewBox="0 0 16 16" fill="currentColor" className={className} style={style}>
      <path d="M3.22 9.78a.75.75 0 010-1.06l4.25-4.25a.75.75 0 011.06 0l4.25 4.25a.75.75 0 01-1.06 1.06L8 6.06 4.28 9.78a.75.75 0 01-1.06 0z" />
    </svg>
  );
}

/** Octicon arrow-left */
export function ArrowLeftIcon({ size = 14, className, style }: IconProps) {
  return (
    <svg width={size} height={size} viewBox="0 0 16 16" fill="currentColor" className={className} style={style}>
      <path d="M7.78 12.53a.75.75 0 0 1-1.06 0L2.47 8.28a.75.75 0 0 1 0-1.06l4.25-4.25a.75.75 0 0 1 1.06 1.06L4.81 7h7.44a.75.75 0 0 1 0 1.5H4.81l2.97 2.97a.75.75 0 0 1 0 1.06Z" />
    </svg>
  );
}

/** Octicon X close (16x16, filled) */
export function CloseIcon16({ size = 14, className, style }: IconProps) {
  return (
    <svg width={size} height={size} viewBox="0 0 16 16" fill="currentColor" className={className} style={style}>
      <path d="M3.72 3.72a.75.75 0 0 1 1.06 0L8 6.94l3.22-3.22a.75.75 0 1 1 1.06 1.06L9.06 8l3.22 3.22a.75.75 0 1 1-1.06 1.06L8 9.06l-3.22 3.22a.75.75 0 0 1-1.06-1.06L6.94 8 3.72 4.78a.75.75 0 0 1 0-1.06Z" />
    </svg>
  );
}

/** Octicon info circle */
export function InfoIcon({ size = 14, className, style }: IconProps) {
  return (
    <svg width={size} height={size} viewBox="0 0 16 16" fill="currentColor" className={className} style={style}>
      <path d="M0 8a8 8 0 1 1 16 0A8 8 0 0 1 0 8Zm8-6.5a6.5 6.5 0 1 0 0 13 6.5 6.5 0 0 0 0-13ZM6.5 7.75A.75.75 0 0 1 7.25 7h1a.75.75 0 0 1 .75.75v2.75h.25a.75.75 0 0 1 0 1.5h-2a.75.75 0 0 1 0-1.5h.25v-2h-.25a.75.75 0 0 1-.75-.75ZM8 6a1 1 0 1 1 0-2 1 1 0 0 1 0 2Z" />
    </svg>
  );
}

/** Octicon warning triangle */
export function WarningIcon({ size = 14, className, style }: IconProps) {
  return (
    <svg width={size} height={size} viewBox="0 0 16 16" fill="currentColor" className={className} style={style}>
      <path d="M6.457 1.047c.659-1.234 2.427-1.234 3.086 0l6.082 11.378A1.75 1.75 0 0 1 14.082 15H1.918a1.75 1.75 0 0 1-1.543-2.575ZM8 5a.75.75 0 0 0-.75.75v2.5a.75.75 0 0 0 1.5 0v-2.5A.75.75 0 0 0 8 5Zm0 9a1 1 0 1 0 0-2 1 1 0 0 0 0 2Z" />
    </svg>
  );
}

/** Octicon error circle */
export function ErrorIcon({ size = 14, className, style }: IconProps) {
  return (
    <svg width={size} height={size} viewBox="0 0 16 16" fill="currentColor" className={className} style={style}>
      <path d="M2.343 13.657A8 8 0 1 1 13.657 2.343 8 8 0 0 1 2.343 13.657ZM6.03 4.97a.751.751 0 0 0-1.042.018.751.751 0 0 0-.018 1.042L6.94 8 4.97 9.97a.749.749 0 0 0 .326 1.275.749.749 0 0 0 .734-.217L8 9.06l1.97 1.97a.749.749 0 0 0 1.275-.326.749.749 0 0 0-.215-.734L9.06 8l1.97-1.97a.749.749 0 0 0-.326-1.275.749.749 0 0 0-.734.217L8 6.94Z" />
    </svg>
  );
}

/** Shield check — for protected workspaces */
export function ShieldCheckIcon({ size = 14, className, style }: IconProps) {
  return (
    <svg width={size} height={size} viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" className={className} style={style}>
      <path d="M8 1L2 4v4c0 3.3 2.6 6.4 6 7 3.4-.6 6-3.7 6-7V4L8 1z" />
      <path d="M5.5 8.5l2 2 3.5-4" />
    </svg>
  );
}

/** Folder with plus — new workspace */
export function FolderPlusIcon({ size = 12, className, style }: IconProps) {
  return (
    <svg width={size} height={size} viewBox="0 0 16 16" fill="none" className={className} style={style}>
      <path d="M1.5 3.5A1 1 0 012.5 2.5h3l1.5 2h6.5a1 1 0 011 1v7a1 1 0 01-1 1h-11a1 1 0 01-1-1v-9z" stroke="currentColor" strokeWidth="1.2" strokeLinejoin="round" />
      <path d="M8 7v4M6 9h4" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round" />
    </svg>
  );
}

/** Extensions — 2×2 grid of rounded squares (nav + titlebar entry). */
/** A single puzzle piece — the "extensions" affordance (matches the browser idiom). */
export function ExtensionsIcon({ size = 14, className, style }: IconProps) {
  return <RawIcon raw={extensionsSvg} size={size} className={className} style={style} />;
}

/** Generic plugin fallback when a plugin ships no icon. Same puzzle piece. */
export function PuzzleIcon({ size = 18, className, style }: IconProps) {
  return <RawIcon raw={extensionsSvg} size={size} className={className} style={style} />;
}

/** Pushpin — pin/unpin a plugin to the titlebar tray. Filled when pinned. */
export function PinIcon({ size = 14, className, style, filled }: IconProps & { filled?: boolean }) {
  return <RawIcon raw={pinSvg} size={size} fill={filled} className={className} style={style} />;
}
