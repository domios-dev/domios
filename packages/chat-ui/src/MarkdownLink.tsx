import type { ComponentProps } from "react";

type LinkOpener = (url: string) => Promise<unknown>;
let opener: LinkOpener | null = null;

export function configureMarkdownLinkOpener(next: LinkOpener | null): void {
  opener = next;
}

export async function openMarkdownLink(href: string): Promise<void> {
  const url = new URL(href);
  if (url.protocol !== "http:" && url.protocol !== "https:") {
    throw new Error("Only HTTP and HTTPS markdown links can be opened");
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
