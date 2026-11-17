/** `canvas` names the `<ds-canvas src>` file that lists `path` as an item. */
export type HtmlFileLoader = (path: string, tabId?: string, canvas?: string) => Promise<string>;

let loader: HtmlFileLoader | null = null;

export function configureHtmlFileLoader(next: HtmlFileLoader | null): void {
  loader = next;
}

export async function loadHtmlFile(path: string, tabId?: string, canvas?: string): Promise<string> {
  if (!loader) {
    throw new Error("File-backed HTML preview is unavailable");
  }
  return canvas === undefined ? loader(path, tabId) : loader(path, tabId, canvas);
}
