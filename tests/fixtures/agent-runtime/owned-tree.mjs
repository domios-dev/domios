import { spawn } from "node:child_process";
import { createInterface } from "node:readline";
import { MessageChannel } from "node:worker_threads";

const role = process.argv[2];
if (role === "leaf") {
  const keepAlive = new MessageChannel();
  keepAlive.port1.on("message", (value) => keepAlive.port2.postMessage(value));
  process.send({ pids: [process.pid] });
} else if (role === "command") {
  const child = spawn(process.execPath, [import.meta.filename, "leaf"], {
    stdio: ["ignore", "ignore", "inherit", "ipc"],
  });
  child.once("message", ({ pids }) => {
    process.send({ pids: [process.pid, ...pids] });
  });
}

if (!role) {
  createInterface({ input: process.stdin }).on("line", (line) => {
    const frame = JSON.parse(line);
    const command = spawn(process.execPath, [import.meta.filename, "command"], {
      stdio: ["ignore", "ignore", "inherit", "ipc"],
    });
    command.once("message", ({ pids }) => {
      process.stdout.write(JSON.stringify({ id: frame.id, result: { pids } }) + "\n");
    });
  });
}
