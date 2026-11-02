import {readFileSync} from 'node:fs';

const store = readFileSync(new URL('../../src-tauri/crates/codeterm-pty/src/session/store.rs', import.meta.url), 'utf8');
const sessionKey = store.match(/pub const LEAF_SESSION_ID_KEY: &str = "([a-z_]+)";/)?.[1];
const leaf = store.split('pub enum SplitNodeSnapshot {')[1]?.split('\n    Split {')[0];
if (!sessionKey || !leaf || !new RegExp(`\\b${sessionKey}: Option<String>`).test(leaf)) {
  throw new Error('Snapshot session field contract is unavailable; inspect SplitNodeSnapshot::Leaf');
}

export const leafSessionIdKey = sessionKey;
