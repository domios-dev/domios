import { client } from "./lib/env.mjs";
import { requireCdp } from "./lib/setup.mjs";

try {
  const { api } = client();
  requireCdp(await api("/api/health"));
} catch (error) {
  console.error(`FAIL prerequisite: ${error.message}`);
  process.exitCode = 7;
}
