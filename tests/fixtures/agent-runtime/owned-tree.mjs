import { spawn } from "node:child_process";
import { createInterface } from "node:readline";
import { MessageChannel } from "node:worker_threads";

const role = process.argv[2];
const orphan = process.argv.includes("--orphan");
if (role === "leaf") {
  const keepAlive = new MessageChannel();
  keepAlive.port1.on("message", (value) => keepAlive.port2.postMessage(value));
  process.send({ pids: [process.pid] });
} else if (role === "command") {
  const child = spawn(process.execPath, [import.meta.filename, "leaf"], {
    stdio: ["ignore", "ignore", "inherit", "ipc"],
  });
  child.once("message", ({ pids }) => {
    process.send({ pids: [process.pid, ...pids] }, () => {
      if (orphan) process.exit(0);
    });
  });
}

if (!role) {
  createInterface({ input: process.stdin }).on("line", (line) => {
    const frame = JSON.parse(line);
    if (frame.method === "fixture/exit") {
      process.exit(0);
    }
    const args = [import.meta.filename, "command"];
    if (frame.params.orphan) args.push("--orphan");
    const command = spawn(process.execPath, args, {
      stdio: ["ignore", "ignore", "inherit", "ipc"],
    });
    command.once("message", ({ pids }) => {
      const reply = () => {
        process.stdout.write(JSON.stringify({ id: frame.id, result: { pids } }) + "\n");
      };
      if (frame.params.orphan) command.once("exit", reply);
      else reply();
    });
  });
}
