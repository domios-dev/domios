import assert from "node:assert/strict";
import { act } from "react";
import { __setInvokeForTest, __setListenForTest } from "../../lib/bridge";
import { installDom, mount, settle } from "../Jobs/jobsEntry.testdata";
import { daemon } from "../General/generalOverview.testdata";
import { useMeshData, type MeshDataResult } from "./useMeshData";

async function main() {
  installDom();
  localStorage.clear();
  let fail = false;
  __setListenForTest(async () => () => {});
  __setInvokeForTest(async <T,>(command: string): Promise<T> => {
    if (command === "mesh_inventory_list") {
      if (fail) throw new Error("inventory transport unavailable");
      return { resources: [daemon] } as T;
    }
    if (command === "mesh_get_status") return { instance_name: "Local" } as T;
    if (command === "mesh_list_peers") return { paired: [], discovered: [] } as T;
    throw new Error(command);
  });
  let state: MeshDataResult | undefined;
  function Probe() {
    state = useMeshData(0);
    return null;
  }
  const view = await mount(<Probe />);
  try {
    await settle(() => state?.loading === false);
    assert.deepEqual(state?.services.map((service) => service.id), [daemon.id]);
    fail = true;
    await act(async () => { await state?.refresh(); });
    assert.deepEqual(state?.services.map((service) => service.id), [daemon.id]);
    assert.equal(state?.loading, false);
    assert.ok(state?.error);
    fail = false;
    await act(async () => { await state?.refresh(); });
    assert.equal(state?.error, null);
    console.log("inventory failure preserves known rows and successful refresh clears failure");
  } finally {
    await view.close();
    __setInvokeForTest(null);
    __setListenForTest(null);
  }
}
void main().catch((error) => { console.error(error); process.exitCode = 1; });
