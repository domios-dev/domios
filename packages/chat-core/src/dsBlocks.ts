import { DsHtmlBoundary } from "./dsBlocks.html";
import { DS_KINDS, isDsKind, parseDsOpening, validateDsBlock, type DsBlock, type DsKind, type DsOpening } from "./dsBlocks.schema";
export { DS_KINDS, DS_TASK_STATUSES, decodeDsEntities, isDsAbsolutePath, isDsTaskRef, serializeDsBlock } from "./dsBlocks.schema";
export type { DsKind, DsBlock, DsTaskStatus, DsTaskItem, DsTaskSnapshot } from "./dsBlocks.schema";

export type DsLifecycle = "streaming" | "ended" | "cancelled";
export interface DsAuthority { id: string; trusted: boolean }
interface Span { id: string; start: number; end: number }
export interface DsFenceSpan { start: number; bodyStart: number; end: number }
export type DsSegment =
  | (Span & { type: "markdown"; text: string; fences: readonly DsFenceSpan[]; pendingFence?: number })
  | (Span & { type: "block"; block: DsBlock })
  | (Span & { type: "pending" | "malformed"; kind?: DsKind | "mermaid" })
  | (Span & { type: "diagram" | "inert"; text: string });
interface Candidate {
  start: number; restoreProse: number; restoreParts: number; restoreFences: DsFenceSpan[]; inert: boolean; kind?: DsKind; header: string; quote: string; bodyStart?: number;
  opening?: DsOpening; html?: DsHtmlBoundary; invalid: boolean;
  jsonQuote: boolean; jsonEscape: boolean;
}
const HEADER_LIMIT = 16_384;
const INERT_NAMES = ["context", "quoted-context", "quoted_context", "handoff", "provider-handoff", "provider_handoff", "environment_context", "system-reminder", "instructions", "INSTRUCTIONS", "previous_messages", "conversation_history"];

export class DsBlockScanner {
  private source = "";
  private cursor = 0;
  private lineSearch = 0;
  private proseStart = 0;
  private proseFences: DsFenceSpan[] = [];
  private candidate: Candidate | null = null;
  private fence: { marker: string; start: number; bodyStart: number; diagram: boolean; invalid?: boolean } | null = null;
  private inert: string[] = [];
  private inertStart: number | null = null;
  private list = false;
  private inlineTicks = 0;
  private markdownHtml: DsHtmlBoundary | null = null;
  private htmlOpaque = false;
  private literalLine = false;
  private listIndent = 0;
  private listBlank = false;
  private parts: DsSegment[] = [];
  private transient: DsSegment | null = null;
  private lifecycle: DsLifecycle = "streaming";
  private fileCount = 0;
  private revision = 0;
  private authority: DsAuthority;

  constructor(authority: DsAuthority) { this.authority = { ...authority }; }
  get text(): string { return this.source; }
  get version(): number { return this.revision; }
  get state(): DsLifecycle { return this.lifecycle; }
  get segments(): readonly DsSegment[] { return this.parts; }

  reset(authority: DsAuthority): void {
    this.source = ""; this.cursor = 0; this.lineSearch = 0; this.proseStart = 0; this.proseFences = []; this.candidate = null;
    this.fence = null; this.inert = []; this.inertStart = null; this.list = false; this.inlineTicks = 0; this.markdownHtml = null; this.htmlOpaque = false; this.literalLine = false; this.listIndent = 0; this.listBlank = false; this.parts = []; this.transient = null;
    this.lifecycle = "streaming"; this.fileCount = 0; this.revision++;
    this.authority = { ...authority };
  }

  append(chunk: string): readonly DsSegment[] {
    if (this.lifecycle === "cancelled") throw new Error("Reset the ds scanner before appending to a cancelled source");
    this.lifecycle = "streaming";
    this.removeTransient();
    this.source += chunk;
    this.scan();
    this.presentTail();
    this.revision++;
    return this.parts;
  }

  finish(lifecycle: Exclude<DsLifecycle, "streaming"> = "ended"): readonly DsSegment[] {
    if (this.lifecycle === "cancelled") return this.parts;
    this.removeTransient();
    this.scan(true);
    this.lifecycle = lifecycle;
    if (this.candidate) this.candidate.invalid = true;
    if (this.fence) this.fence.invalid = true;
    this.presentTail();
    this.revision++;
    return this.parts;
  }

  private identity(start: number): string { return `${this.authority.id}:ds:${start}`; }
  private removeTransient(): void { if (this.transient) this.parts.pop(); this.transient = null; }
  private prose(end: number): void {
    if (end > this.proseStart) this.parts.push({ type: "markdown", id: this.identity(this.proseStart), start: this.proseStart, end, text: this.source.slice(this.proseStart, end), fences: this.proseFences });
    this.proseStart = end; this.proseFences = [];
  }
  private presentTail(): void {
    const pendingStart = this.candidate?.start ?? (this.fence?.diagram ? this.fence.start : undefined);
    if (this.inertStart !== null) {
      this.transient = { type: "inert", id: this.identity(this.inertStart), start: this.inertStart, end: this.source.length, text: this.source.slice(this.inertStart) };
    } else if (pendingStart !== undefined && this.candidate?.inert) {
      this.prose(pendingStart);
      this.transient = { type: "inert", id: this.identity(pendingStart), start: pendingStart, end: this.source.length, text: this.source.slice(pendingStart) };
    } else if (pendingStart !== undefined) {
      this.prose(pendingStart);
      this.transient = { type: this.candidate?.invalid || this.lifecycle !== "streaming" ? "malformed" : "pending", kind: this.candidate?.kind ?? (this.fence?.diagram ? "mermaid" : undefined), id: this.identity(pendingStart), start: pendingStart, end: this.source.length };
    } else if (this.source.length > this.proseStart) {
      this.transient = { type: "markdown", id: this.identity(this.proseStart), start: this.proseStart, end: this.source.length, text: this.source.slice(this.proseStart), fences: this.proseFences, ...(this.fence ? { pendingFence: this.fence.start } : {}) };
    }
    if (this.transient) this.parts.push(this.transient);
  }

  private scan(final = false): void {
    while (this.cursor < this.source.length) {
      if (this.candidate) { this.scanCandidate(); continue; }
      const start = this.cursor;
      if (start > 0 && this.source[start - 1] !== "\n") {
        const nextLine = this.source.indexOf("\n", Math.max(start, this.lineSearch));
        this.lineSearch = nextLine < 0 ? this.source.length : nextLine + 1;
        if (nextLine < 0) { if (final) this.cursor = this.source.length; break; }
        this.cursor = nextLine + 1; continue;
      }
      const newline = this.source.indexOf("\n", Math.max(start, this.lineSearch));
      this.lineSearch = newline < 0 ? this.source.length : newline + 1;
      const end = newline < 0 ? this.source.length : newline + 1;
      const line = this.source.slice(start, newline < 0 && !final ? Math.min(end, start + 256) : end).replace(/\r?\n$/, "");
      const marker = /^ {0,3}(`{3,}|~{3,})([^\r\n]*)$/.exec(line);
      if (this.fence) {
        if (newline < 0 && !final) break;
        if (marker && marker[1][0] === this.fence.marker[0] && marker[1].length >= this.fence.marker.length && !marker[2].trim()) {
          const fence = this.fence;
          this.fence = null;
          if (fence.diagram) {
            this.prose(fence.start);
            this.parts.push({ type: this.authority.trusted && !fence.invalid ? "diagram" : "inert", id: this.identity(fence.start), start: fence.start, end: end, text: this.source.slice(fence.bodyStart, start) });
            this.proseStart = end;
          } else if (!fence.invalid) this.proseFences.push({ start: fence.start, bodyStart: fence.bodyStart, end });
        } else if (newline < 0 && !final) break;
        this.cursor = end; continue;
      }
      if (marker && !this.inlineTicks && !this.inert.length && !this.markdownHtml) {
        if (newline < 0 && !final) break;
        this.fence = { marker: marker[1], start, bodyStart: end, diagram: marker[2].trim().toLowerCase() === "mermaid" && !this.inert.length && !this.list };
        this.cursor = end; continue;
      }
      const completeLine = newline >= 0 || final;
      const rawHtml = /^ {0,3}<(?:\/?(?:address|article|aside|blockquote|body|caption|center|col|colgroup|dd|details|dialog|dir|div|dl|dt|fieldset|figcaption|figure|footer|form|h[1-6]|head|header|hr|html|iframe|legend|li|link|main|menu|nav|ol|p|pre|script|section|source|style|summary|table|tbody|td|textarea|tfoot|th|thead|title|tr|track|ul)(?=[\t\r\n />])|[!?])/i.test(line);
      if (completeLine && !this.inert.length && !this.inlineTicks && !this.list && !this.markdownHtml && rawHtml) {
        this.markdownHtml = new DsHtmlBoundary();
        this.htmlOpaque = /^ {0,3}<(?:script|style|title|textarea)(?=[\t\r\n />])|^ {0,3}<[!?]/i.test(line);
      }
      const insideHtml = this.markdownHtml !== null;
      const htmlData = this.markdownHtml?.inData !== false;
      const context = /^ {0,3}<\/?([A-Za-z_-]+)(?:\s[^>]*)?>/.exec(line);
      if (completeLine && context && INERT_NAMES.includes(context[1])) {
        if (line.trimStart().startsWith("</")) {
          if (this.inert[this.inert.length - 1] === context[1]) this.inert.pop();
          if (!this.inert.length && this.inertStart !== null) {
            this.parts.push({ type: "inert", id: this.identity(this.inertStart), start: this.inertStart, end, text: this.source.slice(this.inertStart, end) });
            this.proseStart = end; this.inertStart = null; this.inlineTicks = 0;
          }
        } else {
          if (!this.inert.length) { this.prose(start); this.inertStart = start; }
          this.inert.push(context[1]);
          if (line.includes(`</${context[1]}>`)) {
            this.inert.pop();
            if (!this.inert.length && this.inertStart !== null) {
              this.parts.push({ type: "inert", id: this.identity(this.inertStart), start: this.inertStart, end, text: this.source.slice(this.inertStart, end) });
              this.proseStart = end; this.inertStart = null;
            }
          }
        }
      }
      if (completeLine && !line.trim()) {
        this.inlineTicks = 0;
        if (this.listIndent) this.listBlank = true;
        else this.list = false;
      }
      const contentIndent = /^ */.exec(line)![0].length;
      if (line.trim() && this.listBlank) {
        if (contentIndent < this.listIndent) this.list = false;
        this.listBlank = false;
      }
      const listMarker = /^ {0,3}(?:[-+*]\s+|\d+[.)]\s+|> ?)/.exec(line)?.[0];
      if (completeLine && listMarker) { this.list = true; this.listIndent = listMarker.trimStart().startsWith(">") ? 0 : listMarker.length; }
      const indent = /^ {0,3}(?=\S)/.exec(line)?.[0].length ?? 0;
      const literalIndent = listMarker?.length ?? indent;
      const rest = line.slice(literalIndent);
      if (!this.literalLine && !this.htmlOpaque && htmlData && (start === 0 || this.source[start - 1] === "\n") && !this.inert.length && !this.inlineTicks && indent < 4 && !/^(?: {4}|\t)/.test(line) && rest.startsWith("<")) {
        const possible = DS_KINDS.some((kind) => `<ds-${kind}`.startsWith(rest)) || /^<ds-[a-z]+(?=[\t\n\f\r />])/.test(rest);
        const name = /^<ds-([a-z]+)(?=[\t\n\f\r />]|$)/.exec(rest)?.[1];
        if (possible && (name === undefined || isDsKind(name) || DS_KINDS.some((kind) => `<ds-${kind}`.startsWith(rest)))) {
          if (listMarker) { this.list = true; this.listIndent = listMarker.trimStart().startsWith(">") ? 0 : listMarker.length; }
          const restoreProse = this.proseStart; const restoreParts = this.parts.length; const restoreFences = this.proseFences;
          this.prose(start + literalIndent);
          this.candidate = { restoreProse, restoreParts, restoreFences, start: start + literalIndent, inert: this.list || Boolean(listMarker) || insideHtml, kind: undefined, header: "", quote: "", invalid: false, jsonQuote: false, jsonEscape: false };
          this.cursor = start + literalIndent; continue;
        }
      }
      if (newline < 0 && !final) break;
      if (this.markdownHtml) {
        for (const char of this.source.slice(start, end)) this.markdownHtml.feed(char);
        if (this.markdownHtml.inData && (this.htmlOpaque || !line.trim())) {
          this.markdownHtml = null; this.htmlOpaque = false;
        }
      }
      this.literalLine = false;
      if (!this.inert.length && !insideHtml) for (const match of line.matchAll(/(?<!\\)(`+)/g)) {
        if (!this.inlineTicks) this.inlineTicks = match[1].length;
        else if (this.inlineTicks === match[1].length) this.inlineTicks = 0;
      }
      this.cursor = end;
    }
  }

  private scanCandidate(): void {
    const c = this.candidate!;
    while (this.cursor < this.source.length) {
      const char = this.source[this.cursor++];
      if (c.bodyStart === undefined) {
        if (c.header.length <= HEADER_LIMIT) c.header += char;
        else c.invalid = true;
        if (!c.kind) {
          const name = /^<ds-([a-z]+)(?=[\t\n\f\r />])/.exec(c.header)?.[1];
          if (name && isDsKind(name)) c.kind = name;
          else if (!DS_KINDS.some((kind) => `<ds-${kind}`.startsWith(c.header))) {
            this.candidate = null;
            this.proseStart = c.restoreProse; this.parts.length = c.restoreParts; this.proseFences = c.restoreFences;
            this.cursor = this.source.lastIndexOf("\n", c.start - 1) + 1; this.literalLine = true;
            return;
          }
        }
        if (c.header.length > HEADER_LIMIT) c.invalid = true;
        if (c.quote) { if (char === c.quote) c.quote = ""; continue; }
        if (char === '"' || char === "'") { c.quote = char; continue; }
        if (char !== ">") continue;
        const opening = parseDsOpening(c.header);
        if (!opening) {
          c.invalid = true;
          if (c.header.endsWith("/>")) { this.commit(null); return; }
          c.bodyStart = this.cursor;
          c.html = new DsHtmlBoundary(`ds-${c.kind}`);
          continue;
        }
        c.kind = opening.kind; c.opening = opening;
        if (opening.selfClosing) { this.commit(c.invalid ? null : validateDsBlock(opening)); return; }
        c.bodyStart = this.cursor;
        c.html = new DsHtmlBoundary(`ds-${opening.kind}`);
        if (opening.kind !== "html" && opening.kind !== "tasks") c.invalid = true;
        continue;
      }
      if (c.kind === "tasks") {
        if (c.jsonQuote) {
          if (c.jsonEscape) c.jsonEscape = false;
          else if (char === "\\") c.jsonEscape = true;
          else if (char === '"') c.jsonQuote = false;
          continue;
        }
        if (char === '"') { c.jsonQuote = true; continue; }
      }
      if (c.html?.feed(char)) {
        const body = this.source.slice(c.bodyStart, this.cursor - `</ds-${c.kind}>`.length);
        this.commit(c.invalid || c.html.invalid ? null : validateDsBlock(c.opening!, body)); return;
      }
    }
  }

  private commit(block: DsBlock | null): void {
    const c = this.candidate!;
    if (block && !c.inert && this.authority.trusted && ["page", "image", "document"].includes(block.kind)) {
      this.fileCount += block.kind === "image" ? block.sources.length : 1;
      if (this.fileCount > 32) block = null;
    }
    const span = { id: this.identity(c.start), start: c.start, end: this.cursor };
    this.parts.push(c.inert ? { ...span, type: "inert", text: this.source.slice(c.start, this.cursor) } : block ? (this.authority.trusted ? { ...span, type: "block", block } : { ...span, type: "inert", text: this.source.slice(c.start, this.cursor) }) : { ...span, type: "malformed", kind: c.kind });
    this.proseStart = this.cursor; this.lineSearch = this.cursor;
    this.candidate = null;
  }
}

export function parseDsBlocks(source: string, authority: DsAuthority = { id: "source", trusted: true }, lifecycle: DsLifecycle = "ended"): readonly DsSegment[] {
  const scanner = new DsBlockScanner(authority);
  scanner.append(source);
  return lifecycle === "streaming" ? scanner.segments : scanner.finish(lifecycle);
}
