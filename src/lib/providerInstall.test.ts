import { requireProviderInstallLaunch } from "./providerInstall";

const opaqueTab = "install-tab-4f82";
const confirmed = requireProviderInstallLaunch({
  tab_id: opaqueTab,
  launch_state: "command_dispatched",
});
if (confirmed.tab_id !== opaqueTab) throw new Error("confirmed install lost its opaque tab id");

let refusal = "";
try {
  requireProviderInstallLaunch({ tab_id: opaqueTab, launch_state: "tab_created" });
} catch (error) {
  refusal = error instanceof Error ? error.message : String(error);
}
if (!refusal) throw new Error("unacknowledged installer launch was accepted");

console.log("provider install acknowledgement tests passed");
