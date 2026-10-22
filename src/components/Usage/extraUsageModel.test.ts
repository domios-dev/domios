import assert from "node:assert/strict";
import {extraUsageLabel} from "./extraUsageModel";

const extra = {enabled:true,effective_enabled:false,disabled_reason:"out_of_credits",spend_limit_reached:false,used:0,limit:10000,currency:"USD",decimal_places:2,settings_url:"https://example.invalid/settings"};
assert.equal(extraUsageLabel(extra), "Usage credits on · no credits left · $100.00/mo limit");
assert.equal(extraUsageLabel({...extra,enabled:false,disabled_reason:null}), "Usage credits off");
assert.equal(extraUsageLabel({...extra,spend_limit_reached:true}), "Usage credits on · limit reached · $100.00/mo limit");
assert.equal(extraUsageLabel({...extra,disabled_reason:"account_review_pending"}), "Usage credits on · account review pending · $100.00/mo limit");
assert.equal(extraUsageLabel({...extra,effective_enabled:true,disabled_reason:null,used:1234}), "Usage credits on · $12.34 used · $100.00/mo limit");
assert.equal(extraUsageLabel({...extra,decimal_places:0,currency:"JPY",limit:100}), "Usage credits on · no credits left · ¥100/mo limit");
assert.equal(extraUsageLabel({...extra,currency:"opaque"}), "Usage credits on · no credits left · 100.00 opaque/mo limit");
console.log("extraUsageModel: passed");
