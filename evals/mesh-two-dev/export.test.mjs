import test from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
const modulePath = fileURLToPath(new URL('./home-export.py', import.meta.url));
const pythonTest = `
import importlib.util,json,pathlib,sqlite3,tempfile,hashlib
spec=importlib.util.spec_from_file_location('home_export',__import__('sys').argv[1])
m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
with tempfile.TemporaryDirectory() as tmp:
 p=pathlib.Path(tmp)/'sessions.db'
 with sqlite3.connect(p) as db:
  db.executescript('CREATE TABLE session(id TEXT PRIMARY KEY); CREATE TABLE message(id TEXT,session_id TEXT,time_created INTEGER,data TEXT); CREATE TABLE part(id TEXT,session_id TEXT,message_id TEXT,time_created INTEGER,data TEXT);')
  db.executemany('INSERT INTO session VALUES (?)',[('own',),('foreign',)])
  db.executemany('INSERT INTO message VALUES (?,?,?,?)',[('m1','own',1,'{"role":"user","model":{"modelID":"cheap"}}'),('m2','foreign',2,'{"role":"assistant","modelID":"expensive"}')])
  db.execute('INSERT INTO part VALUES (?,?,?,?,?)',('p1','own','m1',1,'{"type":"text","text":"probe"}'))
 binding={'home':'windows','port':7686,'tab_id':'tab','session_id':'own','provider':'opencode','format':'opencode-sqlite','tab_snapshot':{'id':'tab','agent_session_id':'own','agent_type':'opencode'},'metadata':{'session_id':'own','provider':'opencode','file_path':str(p)}}
 before=hashlib.sha256(p.read_bytes()).hexdigest()
 raw,proof=m.export(binding)
 assert b'expensive' not in raw and b'probe' in raw
 assert proof['records']==3 and proof['read_only'] is True
 assert before==hashlib.sha256(p.read_bytes()).hexdigest()
 try: m.export({**binding,'port':7685}); raise AssertionError('PROD accepted')
 except ValueError: pass
 with sqlite3.connect(p) as db: db.execute('INSERT INTO part VALUES (?,?,?,?,?)',('p2','own','m2',2,'{}'))
 try: m.export(binding); raise AssertionError('foreign join accepted')
 except ValueError: pass
 for kind,header in [('codex',{'type':'session_meta','payload':{'id':'own'}}),('claude',{'sessionId':'own','message':{'model':'cheap'}})]:
  path=pathlib.Path(tmp)/(kind+'.jsonl');path.write_text(json.dumps(header)+'\\n')
  data,rows=m.read_jsonl(path,'own',kind); assert len(rows)==1
  try: m.read_jsonl(path,'foreign',kind); raise AssertionError('foreign JSONL accepted')
  except ValueError: pass
 path.write_text('{"bound":{"sid":"own"}}\\n')
 assert len(m.read_jsonl(path,'own','session-jsonl',{'sessionPath':['bound','sid']})[1])==1
 try: m.read_jsonl(path,'own','session-jsonl');raise AssertionError('guessed schema')
 except ValueError: pass
print('session-only export and database immutability verified')
`;
test('home exports read exact JSONL sessions and OpenCode SQLite without writes', () => {
  const result = spawnSync('python3', ['-c', pythonTest, modulePath], { encoding: 'utf8', timeout: 10000, env: { ...process.env, PYTHONDONTWRITEBYTECODE: '1' } });
  assert.equal(result.status, 0, result.stderr || result.error?.message);
  assert.match(result.stdout, /immutability verified/);
});
