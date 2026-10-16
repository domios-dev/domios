export function aggregateVisitMeasures(entries, {tabId, startTime, endTime}) {
  const measures = entries.filter(entry => entry.name.startsWith('chat_start:')
    && entry.detail?.tabId === tabId && entry.startTime >= startTime
    && entry.startTime + entry.duration <= endTime);
  const requests = new Map();
  for (const entry of measures) {
    const id = entry.detail.requestId;
    const request = requests.get(id) ?? {requestId:id, responseBytes:null, stages:{}};
    request.stages[entry.name.slice('chat_start:'.length)] = entry.duration;
    if (entry.detail.responseBytes != null) request.responseBytes = entry.detail.responseBytes;
    requests.set(id,request);
  }
  return {measures, requests:[...requests.values()]};
}
