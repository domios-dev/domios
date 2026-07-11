export interface TerminalReplyFilter {
  push(data: string): string;
  flushStandaloneEsc(): string;
  reset(): void;
}

const ESC = "\x1b";
const BEL = "\x07";
const MAX_BUFFER = 256;

export function createTerminalReplyFilter(): TerminalReplyFilter {
  let pending = "";

  return {
    push(data: string): string {
      pending += data;
      if (pending.length > MAX_BUFFER) {
        const keep = pending.slice(-MAX_BUFFER);
        const flush = pending.slice(0, -MAX_BUFFER);
        pending = keep;
        return flush + drain(false);
      }
      return drain(false);
    },
    flushStandaloneEsc(): string {
      if (pending === ESC) {
        pending = "";
        return ESC;
      }
      return "";
    },
    reset(): void {
      pending = "";
    },
  };

  function drain(flushIncomplete: boolean): string {
    let out = "";
    while (pending.length > 0) {
      const esc = pending.indexOf(ESC);
      if (esc < 0) {
        out += pending;
        pending = "";
        break;
      }
      if (esc > 0) {
        out += pending.slice(0, esc);
        pending = pending.slice(esc);
      }
      const reply = terminalReplyLength(pending);
      if (reply === null) {
        if (!flushIncomplete) break;
        out += pending[0];
        pending = pending.slice(1);
        continue;
      }
      if (reply > 0) {
        pending = pending.slice(reply);
        continue;
      }
      out += pending[0];
      pending = pending.slice(1);
    }
    return out;
  }
}

function terminalReplyLength(data: string): number | null {
  if (!data.startsWith(ESC)) return 0;
  if (data.length === 1) return null;
  if (data.startsWith(`${ESC}]10;`) || data.startsWith(`${ESC}]11;`)) return oscReplyLength(data);
  if (data.startsWith(`${ESC}[?`) || data.startsWith(`${ESC}[>`)) return csiReplyLength(data, ["c", "y"]);
  if (data.startsWith(`${ESC}[`)) return csiReplyLength(data, "R");
  return 0;
}

function oscReplyLength(data: string): number | null {
  const bel = data.indexOf(BEL, 2);
  const st = data.indexOf(`${ESC}\\`, 2);
  const end = [bel >= 0 ? bel + 1 : null, st >= 0 ? st + 2 : null]
    .filter((n): n is number => n !== null)
    .sort((a, b) => a - b)[0];
  return end ?? null;
}

function csiReplyLength(data: string, final: string | readonly string[]): number | null {
  const finals = typeof final === "string" ? [final] : final;
  for (let i = 2; i < data.length; i += 1) {
    const ch = data[i];
    if (finals.includes(ch)) return i + 1;
    if (!isCsiParam(ch) && ch !== "$") return 0;
  }
  return null;
}

function isCsiParam(ch: string): boolean {
  return ch === "?" || ch === ">" || ch === ";" || (ch >= "0" && ch <= "9");
}
