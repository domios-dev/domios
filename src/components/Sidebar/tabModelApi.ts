/** Data seam for the sidebar row's model switch. The switch itself is the
 *  provider's own in-agent command, written to the tab's live PTY — the same
 *  route ChatComposer's `/model` picker uses, so no session is restarted. */
import { invoke } from "../../lib/bridge";

export interface TabModelApi {
  /** `payload` is already formatted from the provider's `model_switch_format`. */
  switchModel(ptyId: string, payload: string): Promise<void>;
}

const defaultApi: TabModelApi = {
  switchModel: async (ptyId, payload) => {
    await invoke("write_pty_string", { ptyId, data: payload, raw: false });
  },
};

let apiOverride: TabModelApi | null = null;

export function getTabModelApi(): TabModelApi {
  return apiOverride ?? defaultApi;
}

export function __setTabModelApiForTest(api: TabModelApi | null): void {
  apiOverride = api;
}
