import test from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
const modulePath = fileURLToPath(new URL('./home-export.py', import.meta.url));
const fixturePath = fileURLToPath(new URL('./fixtures/grok-updates.jsonl', import.meta.url));
const script = `
import importlib.util,pathlib,tempfile,json,urllib.parse,sys
from unittest.mock import patch
spec=importlib.util.spec_from_file_location('home_export',sys.argv[1])
m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
with tempfile.TemporaryDirectory() as tmp:
 home=pathlib.Path(tmp);cwd='/work/qa space';sid='fixture-session'
 source=home/'.grok'/'sessions'/urllib.parse.quote(cwd,safe="~!*'()")/sid/'updates.jsonl'
 source.parent.mkdir(parents=True)
 original=pathlib.Path(sys.argv[2]).read_bytes();source.write_bytes(original)
 binding={'home':'aiderqa','port':7697,'tab_id':'fixture-tab','session_id':sid,'provider':'grok','format':'grok','tab_snapshot':{'id':'fixture-tab','agent_session_id':sid,'agent_type':'grok','cwd':cwd}}
 with patch.object(pathlib.Path,'home',return_value=home):
  raw,proof=m.export(binding)
  assert raw==original and proof['records']==7 and proof['cwd']==cwd
  assert proof['original_path']==str(source) and source.read_bytes()==original
  sibling=source.parent.parent/'newest-session'/'updates.jsonl';sibling.parent.mkdir();sibling.write_bytes(original)
  try: m.export({**binding,'source_path':str(sibling)});raise AssertionError('sibling path accepted')
  except ValueError: pass
  rows=[json.loads(line) for line in original.decode().splitlines()]
  rows[0]['params']['sessionId']='foreign';source.write_text('\\n'.join(json.dumps(row) for row in rows)+'\\n')
  try: m.export(binding);raise AssertionError('foreign session accepted')
  except ValueError: pass
print('exact Grok directory and session fixture verified')
`;
test('Grok home export derives only the bound cwd/session path and refuses sibling records', () => {
  const result = spawnSync('python3', ['-c', script, modulePath, fixturePath], { encoding: 'utf8', timeout: 10000, env: { ...process.env, PYTHONDONTWRITEBYTECODE: '1' } });
  assert.equal(result.status, 0, result.stderr || result.error?.message);
  assert.match(result.stdout, /fixture verified/);
});
