/**
 * WHAT: A returning mobile browser holding a cached DEV service worker and shell picks up the packaged production deploy on reload.
 * WHY: The web mirror precaches "/" and its app asset, so the previous worker can keep serving the dev
 * shell — and its dev badge and dev runtime identity — after production ships to the same origin.
 * ORACLE: On the html element, data-build-mode (served index) and data-sw-version (controlling worker's
 * VERSION reply) read dev then prod, data-runtime-identifier (authenticated /api/health) empties, the
 * "Development build, version ..." badge disappears, and the chat region renders with no "Loading chat…".
 */
import { once } from "node:events";
import { createServer, type Server } from "node:http";
import { expect, test } from "../../helpers/fixtures";

type BuildMode = "dev" | "prod";

const TOKEN = "returning-browser-token";
const CHAT_NAME = "Tab chat: Returning browser";

class ReturningBrowserServer {
  private mode: BuildMode = "dev";
  private readonly server: Server;
  origin = "";

  constructor() {
    this.server = createServer((request, response) => {
      const url = new URL(request.url ?? "/", "http://127.0.0.1");
      const send = (body: string, contentType: string) => {
        response.writeHead(200, {
          "Cache-Control": "no-store",
          "Content-Type": contentType,
          "Service-Worker-Allowed": "/",
        });
        response.end(body);
      };
      if (url.pathname.startsWith("/api/")) {
        if (request.headers.authorization !== `Bearer ${TOKEN}`) {
          response.writeHead(401).end("unauthorized");
          return;
        }
        if (url.pathname === "/api/health") {
          send(JSON.stringify({
            build_sha: `${this.mode}-fixture`,
            instance: { identifier: this.mode === "dev" ? "returning-dev" : "" },
          }), "application/json");
          return;
        }
        if (url.pathname === "/api/tabs/returning/chat-raw/start" && request.method === "POST") {
          send(JSON.stringify({ pending: false, messages: [] }), "application/json");
          return;
        }
        response.writeHead(404).end("missing api fixture");
        return;
      }
      if (url.pathname === "/sw.js") {
        send(serviceWorkerSource(this.mode), "application/javascript");
        return;
      }
      if (url.pathname === `/assets/app-${this.mode}.js`) {
        send(appSource(this.mode), "application/javascript");
        return;
      }
      if (url.pathname === "/" || url.pathname === "/index.html") {
        send(indexSource(this.mode), "text/html");
        return;
      }
      response.writeHead(404).end("missing fixture asset");
    });
  }

  async start(): Promise<void> {
    this.server.listen(0, "127.0.0.1");
    await once(this.server, "listening");
    const address = this.server.address();
    if (!address || typeof address === "string") throw new Error("fixture server has no TCP address");
    this.origin = `http://127.0.0.1:${address.port}`;
  }

  usePackagedProduction(): void {
    this.mode = "prod";
  }

  async close(): Promise<void> {
    await new Promise<void>((resolve, reject) => {
      this.server.close((error) => error ? reject(error) : resolve());
    });
  }
}

function indexSource(mode: BuildMode): string {
  return `<!doctype html><html data-build-mode="${mode}"><body><main id="root"></main><script type="module" src="./assets/app-${mode}.js"></script></body></html>`;
}

function appSource(mode: BuildMode): string {
  return `
const mode = ${JSON.stringify(mode)};
const token = new URL(location.href).hash.replace(/^#token=/, "");
const headers = { Authorization: \`Bearer \${token}\` };
const root = document.querySelector("#root");
root.innerHTML = '<div role="status">Loading chat…</div>';
const [healthResponse, chatResponse] = await Promise.all([
  fetch("/api/health", { headers }),
  fetch("/api/tabs/returning/chat-raw/start", { method: "POST", headers }),
]);
if (!healthResponse.ok || !chatResponse.ok) throw new Error("authenticated fixture boot failed");
const health = await healthResponse.json();
const chat = await chatResponse.json();
document.documentElement.dataset.runtimeIdentifier = health.instance.identifier;
root.innerHTML = mode === "dev"
  ? '<div role="status" aria-label="Development build, version fixture">DEV</div>'
  : "";
if (!chat.pending) {
  root.insertAdjacentHTML("beforeend", '<section role="region" aria-label="${CHAT_NAME}">Ready chat</section>');
}

let refreshing = false;
const hadController = Boolean(navigator.serviceWorker.controller);
navigator.serviceWorker.addEventListener("controllerchange", () => {
  if (!hadController || refreshing) return;
  refreshing = true;
  location.reload();
});
const registration = await navigator.serviceWorker.register("/sw.js", { scope: "/", updateViaCache: "none" });
const activate = (worker) => {
  if (worker?.state === "installed" && navigator.serviceWorker.controller) {
    worker.postMessage({ type: "SKIP_WAITING" });
  }
};
registration.addEventListener("updatefound", () => {
  const worker = registration.installing;
  worker?.addEventListener("statechange", () => activate(worker));
});
registration.waiting?.postMessage({ type: "SKIP_WAITING" });
await navigator.serviceWorker.ready;
if (!navigator.serviceWorker.controller) {
  await new Promise((resolve) => navigator.serviceWorker.addEventListener("controllerchange", resolve, { once: true }));
}
if (!refreshing && navigator.serviceWorker.controller) {
  const channel = new MessageChannel();
  const version = new Promise((resolve) => { channel.port1.onmessage = (event) => resolve(event.data.version); });
  navigator.serviceWorker.controller.postMessage({ type: "VERSION" }, [channel.port2]);
  document.documentElement.dataset.swVersion = await version;
}
`;
}

function serviceWorkerSource(mode: BuildMode): string {
  return `
const version = ${JSON.stringify(mode)};
const cacheName = \`returning-browser-\${version}\`;
const appAsset = \`/assets/app-\${version}.js\`;
self.addEventListener("install", (event) => {
  event.waitUntil(caches.open(cacheName).then((cache) => cache.addAll(["/", appAsset])));
});
self.addEventListener("activate", (event) => {
  event.waitUntil(caches.keys()
    .then((keys) => Promise.all(keys.filter((key) => key !== cacheName).map((key) => caches.delete(key))))
    .then(() => self.clients.claim()));
});
self.addEventListener("message", (event) => {
  if (event.data?.type === "SKIP_WAITING") self.skipWaiting();
  if (event.data?.type === "VERSION") event.ports[0]?.postMessage({ version });
});
self.addEventListener("fetch", (event) => {
  const url = new URL(event.request.url);
  if (url.pathname.startsWith("/api/") || url.pathname === "/sw.js") return;
  if (event.request.mode === "navigate") {
    event.respondWith(fetch(event.request).catch(() => caches.match("/")));
    return;
  }
  event.respondWith(caches.match(event.request).then((cached) => cached ?? fetch(event.request)));
});
`;
}

test("@golden returning mobile browser replaces cached DEV shell with packaged production", async ({ webBrowser }) => {
  const fixture = new ReturningBrowserServer();
  await fixture.start();
  const context = await webBrowser.newContext({ viewport: { width: 390, height: 844 } });
  const page = await context.newPage();
  try {
    await page.goto(`${fixture.origin}/#token=${TOKEN}`, { waitUntil: "domcontentloaded" });
    await expect(page.locator("html")).toHaveAttribute("data-sw-version", "dev");
    await expect(page.getByLabel("Development build, version fixture")).toHaveCount(1);
    await expect(page.getByRole("region", { name: CHAT_NAME })).toBeVisible();
    await expect(page.getByText("Loading chat…", { exact: true })).toHaveCount(0);

    fixture.usePackagedProduction();
    await page.reload({ waitUntil: "domcontentloaded" });
    await expect(page.locator("html")).toHaveAttribute("data-build-mode", "prod");
    await expect(page.locator("html")).toHaveAttribute("data-sw-version", "prod");
    await expect(page.locator("html")).toHaveAttribute("data-runtime-identifier", "");
    await expect(page.getByLabel(/Development build, version /)).toHaveCount(0);
    await expect(page.getByRole("region", { name: CHAT_NAME })).toBeVisible();
    await expect(page.getByText("Loading chat…", { exact: true })).toHaveCount(0);
  } finally {
    await context.close();
    await fixture.close();
  }
});
