import type { NextConfig } from "next";

const config: NextConfig = {
  experimental: { authInterrupts: true },
  async headers() {
    return [{ source: "/:path*", headers: [
      { key: "Cache-Control", value: "private, no-store" },
      { key: "X-Content-Type-Options", value: "nosniff" },
      { key: "Referrer-Policy", value: "no-referrer" },
      { key: "X-Frame-Options", value: "DENY" },
    ] }];
  },
};
export default config;
