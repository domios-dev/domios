import type { ComponentProps } from "react";
import { defaultUrlTransform } from "react-markdown";

type LinkOpener = (url: string) => Promise<unknown>;
let opener: LinkOpener | null = null;
let allowsInternalLink: ((url: string) => boolean) | null = null;

export function configureMarkdownLinkOpener(next: LinkOpener | null, allowsInternal?: (url: string) => boolean): void {
  opener = next;
  allowsInternalLink = next ? allowsInternal ?? null : null;
}

export function markdownUrlTransform(url: string, key: string): string {
  return key === "href" && allowsInternalLink?.(url) ? url : defaultUrlTransform(url);
}

export async function openMarkdownLink(href: string): Promise<void> {
  const url = new URL(href);
  if (url.protocol !== "http:" && url.protocol !== "https:" && !allowsInternalLink?.(href)) {
    throw new Error("This markdown link is not allowed");
  }
  if (opener) await opener(href);
  else window.open(href, "_blank", "noopener,noreferrer");
}

export function MarkdownLink({ href, onClick, onAuxClick, ...props }: ComponentProps<"a">) {
  function activate(event: React.MouseEvent<HTMLAnchorElement>) {
    if (event.type === "auxclick" && event.button !== 1) return;
    if (event.type === "auxclick") onAuxClick?.(event);
    else onClick?.(event);
    const handled = event.defaultPrevented;
    event.preventDefault();
    if (!handled && href) void openMarkdownLink(href).catch(() => {});
  }

  return <a {...props} href={href} target="_blank" rel="noopener noreferrer" onClick={activate} onAuxClick={activate} />;
}
