import { __setInvokeForTest } from "../../lib/bridge";
import { materializeDroppedPaths } from "./useTauriFileDrop";

let passed = 0;
let failed = 0;

function assertEq<T>(label: string, actual: T, expected: T) {
  const a = JSON.stringify(actual);
  const e = JSON.stringify(expected);
  if (a === e) {
    console.log(`  ✓ ${label}`);
    passed++;
  } else {
    console.error(`  ✗ ${label}: expected ${e}, got ${a}`);
    failed++;
  }
}

async function test(name: string, fn: () => Promise<void>) {
  try {
    await fn();
    console.log(`✓ ${name}`);
  } catch (error) {
    failed++;
    console.error(`✗ ${name}`);
    console.error(error);
  } finally {
    __setInvokeForTest(null);
  }
}

console.log("useTauriFileDrop:");

async function main() {
  await test("materializes native dropped paths through the backend", async () => {
    const calls: Array<{ cmd: string; args?: Record<string, unknown> }> = [];
    __setInvokeForTest(async (cmd, args) => {
      calls.push({ cmd, args });
      return ["/durable/drop.png"] as unknown as never;
    });

    const paths = await materializeDroppedPaths(["/var/folders/x/T/TemporaryItems/NSIRD_screencaptureui_1.png"]);

    assertEq("returns durable backend path", paths, ["/durable/drop.png"]);
    assertEq("uses materialize command", calls, [{
      cmd: "materialize_dropped_files",
      args: { paths: ["/var/folders/x/T/TemporaryItems/NSIRD_screencaptureui_1.png"] },
    }]);
  });

  await test("falls back to raw paths if backend materialization is unavailable", async () => {
    __setInvokeForTest(async () => {
      throw new Error("unavailable");
    });

    const raw = ["/Users/me/file.txt"];
    assertEq("raw paths preserved", await materializeDroppedPaths(raw), raw);
  });

  if (failed > 0) {
    throw new Error(`${failed} useTauriFileDrop test(s) failed`);
  }

  console.log(`useTauriFileDrop tests: ${passed}/${passed + failed} assertions passed`);
}

void main();
