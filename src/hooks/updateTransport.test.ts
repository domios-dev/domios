import { nativeBridge } from "../lib/nativeBridge";
import { configureDaemonTransport } from "../lib/daemonBootstrap";
import { checkForUpdate, getUpdateStatus, installAppUpdate, requestUpdateChannel } from "./updateTransport";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const calls: Array<{ command: string; args?: Record<string, unknown> }> = [];
const response = { fixture: "opaque" };

nativeBridge.updater = {
  check: async (force) => { calls.push({ command: "check_for_update", args: { force } }); return response; },
  status: async () => { calls.push({ command: "get_update_status" }); return response; },
  install: async () => { calls.push({ command: "install_app_update" }); return { status: "already_running" }; },
};

async function main() {
  const previousFetch = globalThis.fetch;
  try {
    assert((await checkForUpdate(true)) === response, "update checks return the IPC response");
    assert((await getUpdateStatus()) === response, "status reads return the IPC response");
    await installAppUpdate();

    assert(calls[0]?.command === "check_for_update", "update checks use native IPC");
    assert(calls[0]?.args?.force === true, "forced update checks preserve the force flag");
    assert(calls[1]?.command === "get_update_status", "status reads use native IPC");
    assert(calls[2]?.command === "install_app_update", "update installs use native IPC");
    const requests: Array<{ url: string; init: RequestInit }> = [];
    globalThis.fetch = async (input, init = {}) => {
      requests.push({ url: String(input), init });
      return new Response("{}");
    };
    const api = { host: "0.0.0.0", port: 7697, token: "daemon-token" };
    configureDaemonTransport({origin: "http://127.0.0.1:7697", token: "daemon-token"});
    await requestUpdateChannel(api);
    await requestUpdateChannel(api, "rc");
    assert(requests.every((request) => request.url === "http://127.0.0.1:7697/api/update/channel"),
      "native channel requests use the configured daemon port");
    assert(requests.every((request) => new Headers(request.init.headers).get("Authorization") === "Bearer daemon-token"),
      "native channel requests authenticate with the daemon token");
    assert(requests[1].init.method === "POST" && JSON.parse(String(requests[1].init.body)).channel === "rc",
      "channel changes preserve the requested value");
  } finally {
    globalThis.fetch = previousFetch;
    nativeBridge.updater = undefined;
  }
}

void main().then(() => console.log("✓ updater transport stays on native IPC"));
