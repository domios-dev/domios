import assert from 'node:assert/strict';
import net from 'node:net';
import test from 'node:test';
import { serverAddress, serverIsListening } from '../sccache-server-probe.mjs';

test('stats and wrapper use the configured TCP or Unix endpoint', () => {
  assert.deepEqual(serverAddress({}), { host: '127.0.0.1', port: 4226 });
  assert.equal(serverAddress({ SCCACHE_SERVER_PORT: '4227' }).port, 4227);
  assert.equal(serverAddress({ SCCACHE_SERVER_PORT: 'invalid' }).port, 4226);
  assert.equal(serverAddress({ SCCACHE_SERVER_PORT: '65536' }).port, 4226);
  assert.equal(serverAddress({ SCCACHE_SERVER_UDS: '/fixture.sock' }, 'linux').path, '/fixture.sock');
  assert.equal(serverAddress({ SCCACHE_SERVER_UDS: '@fixture' }, 'linux').path, '\0fixture');
  assert.equal(serverAddress({ SCCACHE_SERVER_UDS: '/ignored', SCCACHE_SERVER_PORT: '4227' }, 'win32').port, 4227);
});

test('an actual listener is required even when diagnostic stats could describe S3', async () => {
  const server = net.createServer(socket => socket.end());
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  const address = { host: '127.0.0.1', port: server.address().port };
  assert.equal(await serverIsListening(address), true);
  await new Promise(resolve => server.close(resolve));
  assert.equal(await serverIsListening(address), false);
});
