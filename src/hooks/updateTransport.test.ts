import { __setInvokeForTest } from "../lib/bridge";
import { checkForUpdate, getUpdateStatus, installAppUpdate } from "./updateTransport";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const calls: Array<{ command: string; args?: Record<string, unknown> }> = [];
const response = { fixture: "opaque" };

__setInvokeForTest(async <T>(command: string, args?: Record<string, unknown>) => {
  calls.push({ command, args });
  return response as T;
});

async function main() {
  try {
    assert((await checkForUpdate(true)) === response, "update checks return the IPC response");
    assert((await getUpdateStatus()) === response, "status reads return the IPC response");
    await installAppUpdate();

    assert(calls[0]?.command === "check_for_update", "update checks use native IPC");
    assert(calls[0]?.args?.force === true, "forced update checks preserve the force flag");
    assert(calls[1]?.command === "get_update_status", "status reads use native IPC");
    assert(calls[2]?.command === "install_app_update", "update installs use native IPC");
  } finally {
    __setInvokeForTest(null);
  }
}

void main().then(() => console.log("✓ updater transport stays on native IPC"));
