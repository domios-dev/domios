export class DsHtmlBoundary {
  private token = "";
  private quote = "";
  private truncated = false;
  private depth = 1;
  private raw = "";
  private probe = "";
  private rawClosing = false;
  invalid = false;
  constructor(private readonly outerName = "ds-html") {}
  get inData(): boolean { return !this.token && !this.raw && !this.probe; }

  feed(char: string): boolean {
    if (this.raw && !this.rawClosing) {
      const expected = `</${this.raw}`;
      if (!this.probe) { if (char === "<") this.probe = char; return false; }
      this.probe += char;
      const lower = this.probe.toLowerCase();
      if (expected.startsWith(lower)) return false;
      if (lower.startsWith(expected) && /[\t\n\f\r />]/.test(char)) {
        this.token = this.probe;
        this.probe = "";
        this.rawClosing = true;
        if (char === ">") this.endToken();
      } else this.probe = char === "<" ? "<" : "";
      return false;
    }
    if (!this.token) { if (char === "<") this.token = char; return false; }
    if (this.token.length < 1024) this.token += char;
    else this.truncated = true;
    if (this.token === "<<") { this.token = "<"; return false; }
    if (this.token.length === 2 && !/[a-zA-Z/!?]/.test(char)) { this.token = ""; return false; }
    if (this.token.startsWith("<![CDATA[")) {
      if (this.token.endsWith("]]>")) this.token = "";
      else if (this.token.length > 12) this.token = `<![CDATA[${this.token.slice(-2)}`;
      return false;
    }
    if (this.token.startsWith("<!--")) {
      if (this.token === "<!-->" || this.token.endsWith("-->") || this.token.endsWith("--!>")) this.token = "";
      else if (this.token.length > 8) this.token = `<!--${this.token.slice(-3)}`;
      return false;
    }
    if (this.quote) { if (char === this.quote) this.quote = ""; return false; }
    if (char === '"' || char === "'") { this.quote = char; return false; }
    if (char !== ">") return false;
    return this.endToken();
  }

  private endToken(): boolean {
    const token = this.token;
    const truncated = this.truncated;
    this.token = ""; this.truncated = false;
    if (this.rawClosing) { this.rawClosing = false; this.raw = ""; return false; }
    if (!truncated && token === `</${this.outerName}>`) {
      this.depth--;
      return this.depth === 0;
    }
    const name = /^<([a-zA-Z][a-zA-Z0-9-]*)(?=[\t\n\f\r />])/.exec(token)?.[1]?.toLowerCase();
    if (name === this.outerName) { this.invalid = true; if (!token.endsWith("/>")) this.depth++; }
    if (name && ["script", "style", "title", "textarea"].includes(name)) this.raw = name;
    return false;
  }
}
