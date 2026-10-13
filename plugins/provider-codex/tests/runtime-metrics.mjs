import {readFileSync} from "node:fs";
const source=process.argv[2];
if (!source) throw new Error("Usage: node runtime-metrics.mjs <captured-fixture.json>");
const fixture=JSON.parse(readFileSync(source,"utf8"));
const notifications=fixture.rpc.filter(entry=>entry.frame?.kind === "notification");
const times=notifications.map(entry=>entry.receiptMs);
const start=fixture.rpc[0].receiptMs;
const end=fixture.rpc.at(-1).receiptMs;
function maximum(duration) {
  const starts=[start,...times].filter(time=>time+duration<=end);
  if (!starts.length) return null;
  const count=from=>times.filter(time=>time>=from && time<from+duration).length;
  const from=starts.reduce((best,time)=>count(time)>count(best) ? time : best,starts[0]);
  return {fromMs:from,toMs:from+duration,framesPerSecond:count(from)/(duration/1000)};
}
const sustained=maximum(10000);
const meanBytes=notifications.reduce((sum,entry)=>sum+Buffer.byteLength(JSON.stringify({method:entry.frame.method,params:entry.frame.params}),"utf8"),0)/notifications.length;
console.log(JSON.stringify({source,notificationFrames:notifications.length,intervalMs:{from:start,to:end},peakOneSecond:maximum(1000),sustainedTenSecond:sustained,meanNotificationWireBytes:meanBytes,quickJsMinimumFramesPerSecond:sustained ? sustained.framesPerSecond*2 : null},null,2));
