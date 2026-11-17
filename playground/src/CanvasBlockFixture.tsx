import "./mocks/setup";
import "./mocks/theme";
import "../../src/styles/globals.css";
import { createRoot } from "react-dom/client";
import { RichMarkdown } from "../../packages/chat-ui/src/MarkdownBody";
import { LocalArtifactScope } from "../../packages/chat-ui/src/LocalArtifactScope";
import { configureLocalArtifactLoader } from "../../packages/chat-ui/src/localArtifactLoader";
import { configureHtmlFileLoader } from "../../packages/chat-ui/src/htmlFileLoader";

function screenshot(label: string, hue: number, dense: boolean): string {
  const canvas = document.createElement("canvas");
  canvas.width = 1280; canvas.height = 800;
  const context = canvas.getContext("2d")!;
  context.fillStyle = `hsl(${hue} 30% 12%)`;
  context.fillRect(0, 0, 1280, 800);
  context.fillStyle = `hsl(${hue} 40% 20%)`;
  context.fillRect(0, 0, 1280, dense ? 56 : 88);
  context.fillStyle = `hsl(${hue} 70% 70%)`;
  context.font = "600 34px system-ui";
  context.fillText(label, 40, dense ? 40 : 56);
  for (let row = 0; row < (dense ? 9 : 5); row++) {
    context.fillStyle = `hsl(${hue} 25% ${22 + (row % 2) * 4}%)`;
    context.fillRect(40, 140 + row * (dense ? 66 : 120), 1200, dense ? 52 : 96);
  }
  return canvas.toDataURL("image/png");
}

const files: Record<string, string> = {
  "/work/shots/header-a.png": screenshot("Option A · calm header", 210, false),
  "/work/shots/header-b.png": screenshot("Option B · dense header", 160, true),
};
configureHtmlFileLoader(async () => "<main style='padding:16px'><h2 style='margin:0 0 8px'>Mock page</h2><p>Loaded from a local file and sanitized.</p></main>");

const mock = "<div class='grid gap-3 p-1'><div class='flex items-center justify-between'><h2 class='m-0 text-base font-semibold'>Option B</h2><span class='rounded-full bg-sky-500/20 px-2 py-0.5 text-xs text-sky-300'>denser</span></div><div class='grid grid-cols-3 gap-2'><div class='rounded-lg bg-white/5 p-3'><div class='text-xs opacity-70'>Rows</div><div class='text-2xl font-bold'>+2</div></div><div class='rounded-lg bg-white/5 p-3'><div class='text-xs opacity-70'>Header</div><div class='text-2xl font-bold'>40px</div></div><div class='rounded-lg bg-white/5 p-3'><div class='text-xs opacity-70'>Contrast</div><div class='text-2xl font-bold'>AA</div></div></div><button class='w-fit rounded-md bg-sky-600 px-3 py-1.5 text-sm text-white hover:bg-sky-500'>Approve B</button></div>";
const board = JSON.stringify({
  format: "domios.canvas/1",
  title: "Header options",
  items: [
    { id: "a", kind: "image", src: "/work/shots/header-a.png", x: 0, y: 0, w: 560, h: 350, title: "Option A" },
    { id: "b", kind: "image", src: "/work/shots/header-b.png", x: 600, y: 0, w: 560, h: 350, title: "Option B" },
    { id: "mock", kind: "html", html: mock, x: 0, y: 390, w: 560, h: 260, title: "Option B mock" },
    { id: "note", kind: "note", markdown: "**A** is calmer; **B** fits two more rows.\n\nRecommend **B** with the 48px header.", x: 600, y: 390, w: 360, h: 150 },
    { id: "page", kind: "page", src: "/work/mock/page.html", x: 600, y: 570, w: 360, h: 180, title: "Mock page" },
    { id: "gone", kind: "image", src: "/work/shots/deleted.png", x: 1000, y: 390, w: 160, h: 160, title: "Old shot" },
    { id: "doc", kind: "document", src: "/work/notes/review.md", x: 1000, y: 570, w: 300, h: 180 },
  ],
  views: [{ name: "compare", x: 0, y: 0, w: 1160, h: 350 }, { name: "details", x: 0, y: 390, w: 1300, h: 360 }],
});
const text = `Here are the two header options.\n\n<ds-canvas src="/work/board.canvas.json" view="compare" />\n`;
configureLocalArtifactLoader(async (path, kind) => {
  if (kind === "canvas" && path === "/work/board.canvas.json") return { data: board, media_type: "application/json" };
  if (kind === "image" && files[path]) return { data: files[path], media_type: "image/png" };
  if (kind === "document") return { data: "## Review notes\n\n- A keeps the 48px header.\n- B fits two more rows.\n- Both keep the pager in the header.", media_type: "text/markdown" };
  throw new Error("Preview path is not referenced");
});

const narrow = new URLSearchParams(location.search).has("narrow");
function Fixture() {
  return <main style={{ minHeight: "100dvh", padding: 16, background: "var(--ct-bg)", color: "var(--ct-fg)" }}>
    <div style={{ maxWidth: narrow ? 360 : 760, margin: "0 auto" }} className="text-sm">
      <LocalArtifactScope tabId="fixture"><RichMarkdown text={text} /></LocalArtifactScope>
    </div>
  </main>;
}

createRoot(document.getElementById("root")!).render(<Fixture />);
