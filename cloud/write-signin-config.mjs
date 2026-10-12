import { writeFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";

export function signinConfig(env) {
  const required = ["DOMIOS_FIREBASE_API_KEY", "DOMIOS_FIREBASE_PROJECT_ID", "DOMIOS_FIREBASE_APP_ID", "DOMIOS_FIREBASE_APP_CHECK_SITE_KEY"];
  const missing = required.filter(name => !env[name]?.trim());
  if (missing.length) throw new Error(`Missing sign-in configuration: ${missing.join(", ")}`);
  const projectId = env.DOMIOS_FIREBASE_PROJECT_ID;
  return { provider: "firebase", firebase: {
    apiKey: env.DOMIOS_FIREBASE_API_KEY, authDomain: `${projectId}.firebaseapp.com`,
    projectId, appId: env.DOMIOS_FIREBASE_APP_ID,
  }, appCheckSiteKey: env.DOMIOS_FIREBASE_APP_CHECK_SITE_KEY || "", appleEnabled: env.DOMIOS_APPLE_ENABLED === "true" };
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  try {
    const config = signinConfig(process.env);
    await writeFile(new URL("./hosting/signin/config.json", import.meta.url), JSON.stringify(config, null, 2) + "\n");
  } catch (error) {
    console.error(error instanceof Error ? error.message : "Unable to write sign-in configuration");
    process.exitCode = 1;
  }
}
