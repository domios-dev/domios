import assert from "node:assert/strict";
import { accountNoticeRows,mergeAccountNoticeRows } from "./accountNoticeRows";

const notices=[
  {id:"foreign",provider:"other",session:"own",at_ms:1000,content:"foreign",boundary:null,resets_at_ms:null},
  {id:"foreign-session",provider:"fixture",session:"other",at_ms:1000,content:"foreign",boundary:null,resets_at_ms:null},
  {id:"own",provider:"fixture",session:"own",at_ms:2000,content:"continued",boundary:"turn",resets_at_ms:3000},
];
const projected=accountNoticeRows(notices,"fixture","own");
assert.equal(projected.length,1);
assert.equal(projected[0].systemSubtype,"account_transition");
assert.equal(projected[0].content,"continued");
const before={id:"before",type:"user" as const,content:"opaque input",timestamp:"1000"};
const after={id:"after",type:"assistant" as const,content:"opaque response",timestamp:"3000"};
const rows=[before,after];
const merged=mergeAccountNoticeRows(rows,[...projected,...projected]);
assert.deepEqual(merged.map(row=>row.id),["before","account-notice:own","after"]);
assert.equal(merged[0],before);
assert.equal(merged[2],after);
assert.equal(rows.length,2);
assert.equal(mergeAccountNoticeRows(merged,projected),merged);
assert.equal(mergeAccountNoticeRows(rows,[]),rows);
assert.equal(accountNoticeRows(notices,"fixture","new-session").length,0);
console.log("PASS account notices: own-session projection, chronological merge, deduplication and untouched transcript rows");
