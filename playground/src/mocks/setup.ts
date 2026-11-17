import "./tauri-environment";
import { __setInvokeForTest, __setListenForTest } from "../../../src/lib/bridge";
import { configureLocalPreviews } from "../../../src/lib/configureLocalPreviews";
import { configureDaemonTransport } from "../../../src/lib/daemonBootstrap";
import { ensureDaemonRuntime } from "../../../src/lib/daemonRuntime";
import { mockCommand, mockHttp, mockInvoke, mockListen } from "./backend";

const origin = window.location.origin;
configureDaemonTransport({ origin, token: "gallery-fixture" });
ensureDaemonRuntime();
__setInvokeForTest(mockInvoke);
__setListenForTest(mockListen);
configureLocalPreviews();

mockCommand("mesh_list_peers", () => ({
  paired: [], installations: [], local_instance_name: "Gallery", local_last_active_ms: null, local_dev: true,
}));
mockHttp(origin, "/api/crons", ({ searchParams }) => mockInvoke("cron_list", { workspaceId: searchParams.get("workspace_id") }));
mockHttp(origin, "/api/plans", async ({ searchParams }) => ({
  plans: await mockInvoke(searchParams.has("full") ? "list_plans_full" : "list_plan_index"),
}));
mockHttp(origin, "/api/automation/graph/workspace", ({ searchParams }) =>
  mockInvoke("automation_graph_workspace_snapshot", { workspaceId: searchParams.get("workspace_id") }));
