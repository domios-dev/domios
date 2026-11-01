import type {Host, PluginModule} from "@codeterm/plugin-sdk";
import {RemoteTaskFileStorage, type RemoteTaskCapability, type RemoteTaskDiscovery} from "@codeterm/plugin-sdk";
import {cloudTaskBackend} from "./backend";
import {remoteChatBackend} from "./chatBackend";
import {createCodexCloudTransport} from "./transport";

// The host global is injected after module evaluation in test harnesses, so it is resolved on first use.
export function cloudExports(hostRef: () => Host): Pick<PluginModule, "discoverRemoteTask" | "openSession" | "sendMessage" | "poll" | "closeSession" | "viewCall"> {
  let backend: ReturnType<typeof remoteChatBackend> | undefined;
  let capability: RemoteTaskCapability | undefined;
  let transport: ReturnType<typeof createCodexCloudTransport> | undefined;
  const get = () => {
    if (backend) return backend;
    const host = hostRef();
    const manifest = JSON.parse(host.manifestJson() ?? "{}") as {id: string; capabilities: {remoteTask: RemoteTaskCapability}};
    const dataRoot = host.dataDir();
    if (!dataRoot) throw new Error("Cloud task storage is unavailable");
    const root = `${dataRoot.replace(/[\\/]$/, "")}/codex/tasks`;
    capability = manifest.capabilities.remoteTask;
    transport = createCodexCloudTransport(host);
    backend = remoteChatBackend(new RemoteTaskFileStorage(host.fs, root), cloudTaskBackend(capability, transport), manifest.id);
    return backend;
  };
  const discoverRemoteTask = (): RemoteTaskDiscovery => {
    get();
    const discovery = transport!.discoverRemoteTask();
    Object.assign(capability!, discovery);
    return discovery;
  };
  return {
    discoverRemoteTask,
    openSession: ctx => {
      const discovery = discoverRemoteTask();
      if (discovery.availability.status === "unavailable") throw new Error(discovery.availability.reason);
      return get().openSession(ctx);
    },
    sendMessage: (session, text) => get().sendMessage(session, text),
    poll: (session, cursor) => get().poll(session, cursor),
    closeSession: session => get().closeSession(session),
    viewCall: (method, args, context) => get().viewCall(method, args, context),
  };
}
