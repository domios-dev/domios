import type {Host, PluginModule} from "../../../packages/plugin-sdk/src/index";
import type {RemoteTaskCapability} from "../../../packages/plugin-sdk/src/remoteTasks";
import {RemoteTaskFileStorage} from "../../../packages/plugin-sdk/src/remoteTaskStorage";
import {cloudTaskBackend} from "./backend";
import {remoteChatBackend} from "./chatBackend";

declare const host: Host;
const manifest = JSON.parse(host.manifestJson() ?? "{}") as {id: string; capabilities: {chatBackend: {remoteTask: RemoteTaskCapability}}};
const capability = manifest.capabilities.chatBackend.remoteTask;
const root = host.fs.expandHome("~/.codeterm/codex-cloud/tasks");
if (!root) throw new Error("Remote task storage is unavailable");
const backend = cloudTaskBackend(capability, {
  request() { throw new Error("Live Cloud access has not been verified"); },
});
const plugin: PluginModule = {...remoteChatBackend(new RemoteTaskFileStorage(host.fs, root), backend, manifest.id)};
export default plugin;
