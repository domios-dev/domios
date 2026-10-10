export function jsonLines(text) {
  return text.split(/\r?\n/).filter(line => line.trim()).map((line, index) => {
    try { return JSON.parse(line); }
    catch { throw new Error(`Malformed provider record line ${index + 1}`); }
  });
}
export function at(record, path) {
  return path.reduce((value, field) => value?.[field], record);
}
export function proveRecord(records, claim) {
  if (!claim.sessionId || !claim.tabId || !claim.sourcePath) throw new Error('Bound session and source path required');
  if (!claim.sessionPath?.length || !claim.valuePath?.length) throw new Error('Explicit record paths required');
  const matches = records.flatMap((record, index) => {
    if (at(record, claim.sessionPath) !== claim.sessionId) return [];
    if (at(record, claim.valuePath) !== claim.expected) return [];
    return [{ line: index + 1, record }];
  });
  if (!matches.length) throw new Error('No claim in this tab session');
  return { ...claim, lines: matches.map(match => match.line) };
}
