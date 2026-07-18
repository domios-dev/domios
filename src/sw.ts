/// <reference lib="webworker" />
import {
  cleanupOutdatedCaches,
  matchPrecache,
  precacheAndRoute,
} from "workbox-precaching";
import { registerRoute } from "workbox-routing";
import { NetworkFirst } from "workbox-strategies";

declare const self: ServiceWorkerGlobalScope & {
  __WB_MANIFEST: Array<string | { url: string; revision: string | null }>;
};

precacheAndRoute(self.__WB_MANIFEST);
cleanupOutdatedCaches();

const NETWORK_ONLY = [
  /^\/api\//,
  /\/stream$/,
  /\/read$/,
  /^\/manifest/,
  /^\/upload/,
];

function isNetworkOnly(pathname: string): boolean {
  return NETWORK_ONLY.some((re) => re.test(pathname));
}

registerRoute(
  ({ request, url }) => {
    if (request.mode !== "navigate") return false;
    return !isNetworkOnly(new URL(url).pathname);
  },
  new NetworkFirst({
    cacheName: "codeterm-html",
    networkTimeoutSeconds: 5,
    plugins: [
      {
        handlerDidError: async () =>
          (await matchPrecache("index.html")) ?? Response.error(),
      },
    ],
  }),
);

self.addEventListener("message", (event) => {
  if (event.data?.type === "SKIP_WAITING") {
    void self.skipWaiting();
  }
});

self.addEventListener("activate", (event) => {
  event.waitUntil(self.clients.claim());
});
