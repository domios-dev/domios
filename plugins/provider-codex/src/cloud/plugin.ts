import type {Host, PluginModule} from "@codeterm/plugin-sdk";
import {RemoteTaskFileStorage, type RemoteTaskCapability} from "@codeterm/plugin-sdk";
import {cloudTaskBackend} from "./backend";
import {remoteChatBackend} from "./chatBackend";

// The host global is injected after module evaluation in test harnesses, so it is resolved on first use.
export function cloudExports(hostRef: () => Host): Pick<PluginModule, "openSession" | "sendMessage" | "poll" | "closeSession" | "viewCall"> {
  let backend: ReturnType<typeof remoteChatBackend> | undefined;
  const get = () => {
    if (backend) return backend;
    const host = hostRef();
    const manifest = JSON.parse(host.manifestJson() ?? "{}") as {id: string; capabilities: {remoteTask: RemoteTaskCapability}};
    const root = host.fs.expandHome("~/.codeterm/codex/tasks");
    if (!root) throw new Error("Cloud task storage is unavailable");
    backend = remoteChatBackend(new RemoteTaskFileStorage(host.fs, root), cloudTaskBackend(manifest.capabilities.remoteTask, {
      request() {throw new Error("Live Cloud access has not been verified");},
    }), manifest.id);
    return backend;
  };
  return {
    openSession: ctx => get().openSession(ctx),
    sendMessage: (session, text) => get().sendMessage(session, text),
    poll: (session, cursor) => get().poll(session, cursor),
    closeSession: session => get().closeSession(session),
    viewCall: (method, args) => get().viewCall(method, args),
  };
}
