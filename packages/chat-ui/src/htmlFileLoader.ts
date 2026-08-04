export type HtmlFileLoader = (path: string) => Promise<string>;

let loader: HtmlFileLoader | null = null;

export function configureHtmlFileLoader(next: HtmlFileLoader | null): void {
  loader = next;
}

export async function loadHtmlFile(path: string): Promise<string> {
  if (!loader) {
    throw new Error("File-backed HTML preview is unavailable");
  }
  return loader(path);
}
