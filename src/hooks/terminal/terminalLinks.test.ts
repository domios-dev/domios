import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { Terminal } from "@xterm/xterm";
import {
  findWrappedLinksAtLine,
  normalizeCopiedTerminalSelection,
  openTerminalLink,
  resolveSafeTerminalLinkUrl,
  resolveTerminalLinkTarget,
  type TerminalBufferLike,
} from "./terminalLinks";

const CLAUDE_LOGIN_URL =
  "https://claude.ai/oauth/authorize?code=true&client_id=9d1c250a-e61b-44d9-88ed-5944d1962f5e"
  + "&response_type=code&redirect_uri=https%3A%2F%2Fconsole.anthropic.com%2Foauth%2Fcode%2Fcallback"
  + "&scope=org%3Acreate_api_key+user%3Aprofile+user%3Ainference"
  + "&code_challenge=E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
  + "&code_challenge_method=S256&state=abc123XYZ_789";

async function write(term: Terminal, data: string): Promise<void> {
  await new Promise<void>((resolve) => term.write(data, resolve));
}

describe("terminal link target resolution", () => {
  it("opens http and https links locally for both remote and local tabs", async () => {
    for (const isRemoteTab of [true, false]) {
      for (const url of [CLAUDE_LOGIN_URL, "http://127.0.0.1:8080/callback?ok=1"]) {
        assert.deepEqual(resolveTerminalLinkTarget(url, { isRemoteTab }), {
          action: "open_local",
          url,
        });
        const opened: string[] = [];
        const fallback: string[] = [];
        const ok = await openTerminalLink(url, { isRemoteTab }, {
          openUrl: async (target) => { opened.push(target); },
          fallbackOpen: (target) => { fallback.push(target); },
        });
        assert.equal(ok, true);
        assert.deepEqual(opened, [url]);
        assert.deepEqual(fallback, []);
      }
    }
  });

  it("refuses non-http/https schemes and malformed URLs on remote and local tabs", async () => {
    const unsafe = [
      "javascript:alert(1)",
      "data:text/html,<script>alert(1)</script>",
      "file:///etc/passwd",
      "file:///C:/Windows/win.ini",
      "vbscript:msgbox(1)",
      "mailto:owner@example.test",
      "domios://neighbor/peer/permissions",
      "ssh://green.tailnet",
    ];
    for (const isRemoteTab of [true, false]) {
      for (const uri of unsafe) {
        assert.deepEqual(resolveTerminalLinkTarget(uri, { isRemoteTab }), {
          action: "refuse",
          reason: "disallowed_scheme",
        });
        assert.equal(resolveSafeTerminalLinkUrl(uri), null);
        const opened: string[] = [];
        const ok = await openTerminalLink(uri, { isRemoteTab }, {
          openUrl: async (target) => { opened.push(target); },
          fallbackOpen: (target) => { opened.push(target); },
        });
        assert.equal(ok, false);
        assert.deepEqual(opened, []);
      }
      for (const invalid of ["", "   ", "/relative/path", "https://", "https://example.test/\u0007bell"]) {
        assert.deepEqual(resolveTerminalLinkTarget(invalid, { isRemoteTab }), {
          action: "refuse",
          reason: "invalid_url",
        });
      }
    }
  });

  it("falls back to safe browser opener when native openUrl rejects", async () => {
    const fallback: string[] = [];
    const ok = await openTerminalLink(CLAUDE_LOGIN_URL, { isRemoteTab: true }, {
      openUrl: async () => { throw new Error("native unavailable"); },
      fallbackOpen: (target) => { fallback.push(target); },
    });
    assert.equal(ok, true);
    assert.deepEqual(fallback, [CLAUDE_LOGIN_URL]);
  });
});

describe("wrapped terminal URL detection", () => {
  it("detects a soft-wrapped long OAuth URL whole from every wrapped row", async () => {
    const term = new Terminal({ cols: 80, rows: 12, allowProposedApi: true });
    try {
      await write(term, `Browser didn't open? Use the URL below to sign in (c to copy)\r\n\r\n${CLAUDE_LOGIN_URL}\r\n\r\nHold Shift while selecting to use your terminal's native copy`);
      const buffer = term.buffer.active as unknown as TerminalBufferLike;
      const expectedRows = Math.ceil(CLAUDE_LOGIN_URL.length / 80);
      for (let row = 3; row < 3 + expectedRows; row += 1) {
        const links = findWrappedLinksAtLine(buffer, row, term.cols);
        assert.equal(links.length, 1, `expected 1 link on row ${row}`);
        assert.equal(links[0].url, CLAUDE_LOGIN_URL);
        assert.equal(links[0].range.start.y, 3);
        assert.equal(links[0].range.end.y, 3 + expectedRows - 1);
      }
    } finally {
      term.dispose();
    }
  });

  it("detects a hard-wrapped TUI/Ink OAuth URL whole from every wrapped row, including indentation", async () => {
    const term = new Terminal({ cols: 120, rows: 14, allowProposedApi: true });
    try {
      // Simulate a remote 78-col PTY rendered inside a wider 120-col viewer (explicit CRLF + 2-space indent).
      const chunks: string[] = [];
      for (let i = 0; i < CLAUDE_LOGIN_URL.length; i += 76) {
        chunks.push(`  ${CLAUDE_LOGIN_URL.slice(i, i + 76)}`);
      }
      await write(
        term,
        `  Browser didn't open? Use the URL below to sign in (c to copy)\r\n\r\n${chunks.join("\r\n")}\r\n\r\n  Hold Shift while selecting to use your terminal's native copy`,
      );
      const buffer = term.buffer.active as unknown as TerminalBufferLike;
      for (let row = 3; row < 3 + chunks.length; row += 1) {
        const links = findWrappedLinksAtLine(buffer, row, term.cols);
        assert.equal(links.length, 1, `expected 1 link on hard-wrapped row ${row}`);
        assert.equal(links[0].url, CLAUDE_LOGIN_URL);
        assert.deepEqual(links[0].range.start, { x: 3, y: 3 });
        assert.equal(links[0].range.end.y, 3 + chunks.length - 1);
      }
    } finally {
      term.dispose();
    }
  });

  it("trims sentence punctuation, preserves balanced parens, and does not merge unrelated next lines", async () => {
    const term = new Terminal({ cols: 80, rows: 12, allowProposedApi: true });
    try {
      await write(
        term,
        "See https://en.wikipedia.org/wiki/Foo_(bar).\r\n"
        + "https://example.test/one\r\n"
        + "https://example.test/two\r\n"
        + "https://example.test/path/that/is/reasonably/long/for/testing/done/now\r\n"
        + "Done.\r\n"
        + "https://example.test/short\r\n"
        + "nextcommand\r\n",
      );
      const buffer = term.buffer.active as unknown as TerminalBufferLike;
      assert.equal(findWrappedLinksAtLine(buffer, 1, 80)[0]?.url, "https://en.wikipedia.org/wiki/Foo_(bar)");
      assert.equal(findWrappedLinksAtLine(buffer, 2, 80)[0]?.url, "https://example.test/one");
      assert.equal(findWrappedLinksAtLine(buffer, 3, 80)[0]?.url, "https://example.test/two");
      assert.equal(
        findWrappedLinksAtLine(buffer, 4, 80)[0]?.url,
        "https://example.test/path/that/is/reasonably/long/for/testing/done/now",
      );
      assert.deepEqual(findWrappedLinksAtLine(buffer, 5, 80), []);
      assert.equal(findWrappedLinksAtLine(buffer, 6, 80)[0]?.url, "https://example.test/short");
    } finally {
      term.dispose();
    }
  });
});

describe("copied terminal selection URL normalization", () => {
  it("unwraps a hard-wrapped multi-line URL selection into one complete URL", () => {
    const wrapped = [
      `  ${CLAUDE_LOGIN_URL.slice(0, 76)}`,
      `  ${CLAUDE_LOGIN_URL.slice(76, 152)}`,
      `  ${CLAUDE_LOGIN_URL.slice(152, 228)}`,
      `  ${CLAUDE_LOGIN_URL.slice(228)}`,
    ].join("\r\n");
    assert.equal(normalizeCopiedTerminalSelection(wrapped), CLAUDE_LOGIN_URL);
  });

  it("leaves ordinary multi-line text, code, and multi-URL selections unchanged", () => {
    const code = "const a = 1;\nconst b = 2;";
    assert.equal(normalizeCopiedTerminalSelection(code), code);

    const twoUrls = "https://example.test/a\nhttps://example.test/b";
    assert.equal(normalizeCopiedTerminalSelection(twoUrls), twoUrls);

    const withProse = "https://example.test/a\nHold Shift while selecting";
    assert.equal(normalizeCopiedTerminalSelection(withProse), withProse);

    const withBlankLine = "https://example.test/a\n\ncontinuation";
    assert.equal(normalizeCopiedTerminalSelection(withBlankLine), withBlankLine);
  });
});
