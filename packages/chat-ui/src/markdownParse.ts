import { unified, type Processor } from "unified";
import remarkParse from "remark-parse";
import remarkGfm from "remark-gfm";

export function createMarkdownParser<T>(parse: (source: string) => T, maxChars = 500_000, maxEntries = 3000) {
  const cache = new Map<string, T>();
  let chars = 0;
  return (source: string): T => {
    if (cache.has(source)) {
      const hit = cache.get(source)!;
      cache.delete(source);
      cache.set(source, hit);
      return hit;
    }
    const tree = parse(source);
    if (source.length > Math.min(50_000, maxChars)) return tree;
    cache.set(source, tree);
    chars += source.length;
    while (chars > maxChars || cache.size > maxEntries) {
      const oldest = cache.keys().next().value;
      if (oldest === undefined) break;
      chars -= oldest.length;
      cache.delete(oldest);
    }
    return tree;
  };
}

const parser = unified().use(remarkParse).use(remarkGfm);
export const parsedMarkdown = createMarkdownParser((source: string) => parser.parse(source));

export function remarkCachedParse(this: Processor) {
  // Remark transformers mutate trees; the shared syntax must remain pristine for message-part offsets.
  this.parser = (source: string) => structuredClone(parsedMarkdown(source));
}
