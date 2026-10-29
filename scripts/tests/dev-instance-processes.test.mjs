import test from 'node:test';
import assert from 'node:assert/strict';
import { instanceTreePlan } from '../dev-instance-processes.mjs';

for (const windows of [false, true]) {
  const prefix = windows ? 'C:/repo' : '/repo';
  const root = `${prefix}/src-tauri/target/dev-instances/own`;
  const row = (pid, executable, parent = 1) => ({ pid, executable, parent, created: `${pid}` });
  test(`instance tree selects every own image, including detached orphans (${windows ? 'Windows' : 'Unix'})`, () => {
    const processes = [
      row(1, `${root}/debug/domios-dev-app.exe`),
      row(2, `${root}/debug/domios-server/domios-server.exe`),
      row(3, `${root}/debug/conpty/conpty-agent.exe`, 2),
      row(4, `${root}/debug/domios-server/domios-server.exe`, 999),
      row(5, `${prefix}/src-tauri/target/dev-instances/foreign/debug/domios-dev-app.exe`, 1),
      row(6, `${prefix}/src-tauri/target/debug/domios-dev-app.exe`, 1),
      row(7, `${prefix}/AppData/Local/Domios/domios-server.exe`, 1),
      row(8, `${root}-foreign/debug/domios-dev-app.exe`),
      row(9, `${root}/../foreign/debug/domios-server.exe`),
      row(10, `${prefix}/system/shell`, 2),
    ];
    const result = instanceTreePlan({ root, windows, port: 17996, processes, listeners: [{ port: 7685, pid: 7 }] });
    assert.deepEqual(result, processes.slice(0, 4));
    assert.deepEqual(instanceTreePlan({ root, windows, port: 17996, processes: processes.slice(4), listeners: [] }), []);
  });

  test(`production ownership and unknown creation identity refuse (${windows ? 'Windows' : 'Unix'})`, () => {
    const processes = [row(42, `${root}/debug/server`)];
    assert.throws(() => instanceTreePlan({ root, windows, port: 7685, processes: [], listeners: [] }), /production/);
    assert.throws(() => instanceTreePlan({ root, windows, port: 17996, processes, listeners: [{ port: 7685, pid: 42 }] }), /production/);
    assert.throws(() => instanceTreePlan({ root, windows, port: 17996, processes: [{ ...processes[0], created: '' }], listeners: [] }), /creation identity/);
  });
}

test('Windows image spelling normalizes separators and extended paths; Unix remains case sensitive', () => {
  const root = 'C:/repo/target/dev-instances/own';
  const process = { pid: 42, created: '123', executable: '\\\\?\\C:\\REPO\\target\\dev-instances\\own\\debug\\server.exe' };
  assert.deepEqual(instanceTreePlan({ root, windows: true, port: 17996, processes: [process], listeners: [] }), [process]);
  assert.deepEqual(instanceTreePlan({ root: '/repo/own', windows: false, port: 17996,
    processes: [{ ...process, executable: '/repo/Own/server' }], listeners: [] }), []);
});

test('canonical DEV selection excludes named instance trees and installed production', () => {
  const processes = [
    { pid: 1, created: '1', executable: '/repo/src-tauri/target/debug/domios-dev-app' },
    { pid: 2, created: '2', executable: '/repo/src-tauri/target/dev-instances/own/debug/server' },
    { pid: 3, created: '3', executable: '/opt/Domios/domios-server' },
  ];
  assert.deepEqual(instanceTreePlan({ root: '/repo/src-tauri/target/debug', windows: false, port: 7686,
    processes, listeners: [{ port: 7685, pid: 3 }] }), [processes[0]]);
});
