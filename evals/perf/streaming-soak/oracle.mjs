import {isDeepStrictEqual} from 'node:util';
import {requireValue} from './model.mjs';

export const METRICS = ['jsBytes', 'rendererPrivateBytes', 'daemonPrivateBytes'];
export function median(values) {
  const sorted = [...values].sort((a,b) => a-b), middle = Math.floor(sorted.length/2);
  return sorted.length ? (sorted.length%2 ? sorted[middle] : (sorted[middle-1]+sorted[middle])/2) : null;
}
export function seriesStats(series, metric) {
  if (series.length < 41 || series.some((s,i) => !Number.isFinite(s.at) ||
    s.postGC !== true || !Number.isFinite(s[metric]) || s[metric] < 0 || (i && (s.at <= series[i-1].at || s.at-series[i-1].at > 90_000)))) return null;
  const first = series[0].at, last = series.at(-1).at;
  if (last-first < 40*60_000) return null;
  const early = median(series.filter(s => s.at < first+20*60_000).map(s => s[metric]));
  const late = median(series.filter(s => s.at > last-20*60_000).map(s => s[metric]));
  const meanT = series.reduce((n,s) => n+(s.at-first)/3_600_000,0)/series.length;
  const meanY = series.reduce((n,s) => n+s[metric],0)/series.length;
  const covariance = series.reduce((n,s) => n+((s.at-first)/3_600_000-meanT)*(s[metric]-meanY),0);
  const variance = series.reduce((n,s) => n+((s.at-first)/3_600_000-meanT)**2,0);
  const earlyT = median(series.filter(s => s.at < first+20*60_000).map(s => s.at));
  const lateT = median(series.filter(s => s.at > last-20*60_000).map(s => s.at));
  return {perHour:covariance/variance, early20MinuteMedian:early, late20MinuteMedian:late,
    medianPerHour:(late-early)/((lateT-earlyT)/3_600_000), samples:series.length};
}
export function declareControl(control, match, declaredAt) {
  requireValue(control?.version === 1 && control.mode === 'idle' && control.run &&
    Number.isFinite(control.completedAt) && control.completedAt <= declaredAt &&
    isDeepStrictEqual(control.match,match), 'Matched idle control required up front');
  const allowances = {};
  for (const metric of METRICS) {
    const stats = seriesStats(control.samples,metric);
    requireValue(stats !== null, `Idle control missing ${metric} coverage`);
    allowances[metric] = Math.max(0,stats.perHour,stats.medianPerHour);
  }
  return {run:control.run, declaredAt, match, allowances};
}
export function plateau(samples, declaration) {
  const metrics = Object.fromEntries(METRICS.map(metric => {
    const stats = seriesStats(samples,metric), allowed = declaration?.allowances?.[metric];
    if (!stats || !Number.isFinite(allowed) || allowed < 0) return [metric,{status:'unknown',stats,allowed:allowed??null}];
    return [metric,{status:Math.max(stats.perHour,stats.medianPerHour) <= allowed+1e-6 ? 'flat' : 'rising', allowed, ...stats}];
  }));
  const statuses = Object.values(metrics).map(v => v.status);
  return {status:statuses.includes('rising')?'rising':statuses.includes('unknown')?'unknown':'flat',metrics};
}
