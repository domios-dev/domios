declare module "workbox-precaching" {
  export function cleanupOutdatedCaches(): void;
  export function matchPrecache(path: string): Promise<Response | undefined>;
  export function precacheAndRoute(entries: Array<string | { url: string; revision: string | null }>): void;
}

declare module "workbox-routing" {
  export function registerRoute(
    match: (context: { request: Request; url: URL }) => boolean,
    handler: unknown,
  ): void;
}

declare module "workbox-strategies" {
  export class NetworkFirst {
    constructor(options: {
      cacheName?: string;
      networkTimeoutSeconds?: number;
      plugins?: Array<{
        handlerDidError?: () => Promise<Response>;
      }>;
    });
  }
}
