import { readFileSync } from 'node:fs';
export const targets = Object.freeze(Object.fromEntries(Object.entries(JSON.parse(readFileSync(new URL('./targets.json', import.meta.url), 'utf8'))).map(([name, target]) => [name, Object.freeze(target)])));
export const targetNames = Object.freeze(Object.keys(targets));
export function targetFor(name) {
  if (!Object.hasOwn(targets, name)) throw new Error('Unknown phase-B endpoint');
  return targets[name];
}
export function validateTarget(instance, port) {
  if (port !== targetFor(instance).api_port) throw new Error('Only the assigned phase-B DEV port is allowed');
}
