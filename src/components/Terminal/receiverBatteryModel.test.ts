import assert from "node:assert/strict";
import { connectedReceiverBatteries } from "./receiverBatteryModel";
assert.deepEqual(connectedReceiverBatteries([{slot:1, connected:false, percent:null}, {slot:2, connected:true, percent:97}]), [{slot:2, connected:true, percent:97}]);
assert.equal(connectedReceiverBatteries([{slot:1, connected:true, percent:0}])[0].percent, 0);
for (const percent of [101, -1, 1.5, null, "97"]) assert.deepEqual(connectedReceiverBatteries([{slot:1, connected:true, percent}]), []);
assert.deepEqual(connectedReceiverBatteries([{slot:1, connected:true, percent:50}, {slot:1, connected:true, percent:60}]), []);
assert.deepEqual(connectedReceiverBatteries({percent:97}), []);
console.log("receiver battery: observed, empty, zero and invalid-data fixtures passed");
