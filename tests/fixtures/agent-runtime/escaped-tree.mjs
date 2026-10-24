import { spawn } from "node:child_process";
import { MessageChannel } from "node:worker_threads";

if (process.argv[2] === "leaf") {
  const channel = new MessageChannel();
  channel.port1.on("message", (value) => channel.port2.postMessage(value));
  process.send(process.pid);
} else {
  const child = spawn(process.execPath, [import.meta.filename, "leaf"], {
    stdio: ["ignore", "ignore", "inherit", "ipc"],
  });
  child.once("message", (pid) => {
    process.stdout.write("fixture-tree:" + JSON.stringify([process.pid, pid]) + "\n");
  });
}
