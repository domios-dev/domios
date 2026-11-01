import type { ComponentProps } from "react";
import { defaultUrlTransform } from "react-markdown";

type LinkOpener = (url: string) => Promise<unknown>;
let opener: LinkOpener | null = null;
let allowsInternalLink: ((url: string) => boolean) | null = null;

export function configureMarkdownLinkOpener(next: LinkOpener | null, allowsInternal?: (url: string) => boolean): void {
  opener = next;
  allowsInternalLink = next ? allowsInternal ?? null : null;
}

function localFileUrl(href: string): string | null {
  if (/^[a-z]:[\\/]/i.test(href)) return new URL(`file:///${href.replaceAll("\\", "/")}`).href;
  if (href.startsWith("/") && !href.startsWith("//")) return new URL(`file://${href}`).href;
  if (!href.startsWith("file:")) return null;
  try {
    const url = new URL(href);
    return !url.hostname || url.hostname === "localhost" ? url.href : null;
  } catch {
    return null;
  }
}

export function markdownUrlTransform(url: string, key: string, allowLocalFiles = false): string {
  if (key === "href" && (allowsInternalLink?.(url) || (allowLocalFiles && localFileUrl(url)))) return url;
  return defaultUrlTransform(url);
}

export async function openMarkdownLink(href: string, allowLocalFiles = false): Promise<void> {
  const target = allowLocalFiles ? localFileUrl(href) ?? href : href;
  const url = new URL(target);
  if (url.protocol !== "http:" && url.protocol !== "https:" && !(allowLocalFiles && localFileUrl(target)) && !allowsInternalLink?.(href)) {
    throw new Error("This markdown link is not allowed");
  }
  if (opener) await opener(target);
  else window.open(target, "_blank", "noopener,noreferrer");
}

export function MarkdownLink({ href, onClick, onAuxClick, allowLocalFiles = false, ...props }: ComponentProps<"a"> & { allowLocalFiles?: boolean }) {
  function activate(event: React.MouseEvent<HTMLAnchorElement>) {
    if (event.type === "auxclick" && event.button !== 1) return;
    if (event.type === "auxclick") onAuxClick?.(event);
    else onClick?.(event);
    const handled = event.defaultPrevented;
    event.preventDefault();
    if (!handled && href) void openMarkdownLink(href, allowLocalFiles).catch(() => {});
  }

  return <a {...props} href={href} target="_blank" rel="noopener noreferrer" onClick={activate} onAuxClick={activate} />;
}
