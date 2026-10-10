import { validateTarget, targetNames } from './targets.mjs';
export function verifyPair(endpoints, expected) {
  if (!expected.aiderqa || !expected.windows || expected.aiderqa === expected.windows) throw new Error('Two distinct pinned identities required');
  for (const name of targetNames) {
    const other = targetNames.find(candidate => candidate !== name);
    const { status, peers } = endpoints[name] ?? {};
    if (!status || !peers) throw new Error(`Missing endpoint: ${name}`);
    validateTarget(name, status.api_port);
    if (status.instance_id !== expected[name] || !status.enabled || peers.local_dev !== true) throw new Error(`Wrong DEV identity: ${name}`);
    if (status.paired_count !== 1 || !Array.isArray(peers.paired) || peers.paired.length !== 1) throw new Error(`Not exclusively paired: ${name}`);
    const peer = peers.paired[0];
    if (peer.instance_id !== expected[other] || peer.api_port !== endpoints[other]?.status.api_port || peer.online !== true) throw new Error(`Wrong or offline peer: ${name}`);
    if (peers.incoming_requests?.length) throw new Error(`Unresolved pairing request: ${name}`);
  }
  return { paired: true, identities: { ...expected } };
}
