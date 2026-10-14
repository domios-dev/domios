// Caller: CI rust-warnings-summary.yml
import fs from 'node:fs';
const token = process.env.GH_TOKEN;
const repo = process.env.GITHUB_REPOSITORY;
const sha = process.env.WARNING_SOURCE_SHA;
async function api(route, body) {
  const response = await fetch(`https://api.github.com/repos/${repo}/${route}`, {
    method: body ? 'POST' : 'GET', headers: { Authorization: `Bearer ${token}`, Accept: 'application/vnd.github+json', 'Content-Type': 'application/json' },
    ...(body ? { body: JSON.stringify(body) } : {}) });
  if (!response.ok) throw new Error(`GitHub ${route}: ${response.status}`);
  return response.json();
}
const prs = await api(`commits/${sha}/pulls`);
const report = fs.readFileSync(process.argv[2], 'utf8');
const link = `${process.env.GITHUB_SERVER_URL}/${repo}/actions/runs/${process.env.GITHUB_RUN_ID}`;
for (const pr of prs.filter(pr => pr.head?.ref?.startsWith('release/'))) {
  await api(`issues/${pr.number}/comments`, { body: `Rust warning report: [full report and diagnostics](${link})\n\n` + report.split('\n### Diagnostics')[0].slice(0, 50000) });
}
