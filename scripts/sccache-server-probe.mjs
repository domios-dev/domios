import net from 'node:net';
import { pathToFileURL } from 'node:url';

export function serverAddress(env = process.env, platform = process.platform) {
  if (platform !== 'win32' && env.SCCACHE_SERVER_UDS) {
    const path = env.SCCACHE_SERVER_UDS;
    return { path: path.startsWith('@') ? `\0${path.slice(1)}` : path };
  }
  const raw = env.SCCACHE_SERVER_PORT;
  const port = /^\d+$/.test(raw || '') && Number(raw) <= 65535 ? Number(raw) : 4226;
  return { host: '127.0.0.1', port };
}

export function serverIsListening(address, connect = net.createConnection) {
  return new Promise(resolve => {
    const socket = connect(address);
    const finish = live => { socket.destroy(); resolve(live); };
    socket.once('connect', () => finish(true));
    socket.once('error', () => finish(false));
    socket.setTimeout(2000, () => finish(false));
  });
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  if (process.argv[2] === '--address') console.log(JSON.stringify(serverAddress()));
  else process.exitCode = await serverIsListening(serverAddress()) ? 0 : 1;
}
