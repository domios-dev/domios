import argparse
import json
import os
from pathlib import Path
import platform
import queue
import re
import shlex
import signal
import socket
import subprocess
import sys
import threading
import time
import uuid

from oracles import checked_definition, verify_authored, verify_cards, verify_identity, verify_run
from receipts import digest, redact, save, walk
from subjects import launch, subject_environment, verify_help

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
INSTANCE = 'blueprint-v2-proof'
PORT = 17693


def git(*args):
    return subprocess.check_output(['git', *args], cwd=ROOT, text=True).strip()


def clean_sha():
    if git('status', '--porcelain'):
        raise ValueError('source checkout is dirty')
    return git('rev-parse', 'HEAD')


def config(out, identities=True):
    chair = json.loads((out / 'chair.json').read_text())
    required = ['owner', 'team_id', 'workspace_id', 'orchestrator_tab', 'codex_tab', 'claude_tab', 'no_fit_tab',
                'codex_binary', 'claude_binary', 'claude_model', 'call_sha', 'runtime_main_sha',
                'budget_runtime_sha', 'isolated_dev_sha']
    if not identities:
        required = [key for key in required if not key.endswith('_tab') and key not in ['team_id', 'workspace_id']]
    if any(not chair.get(key) or str(chair[key]).startswith('FILL_') for key in required) or chair['owner'] != 'Chair':
        raise ValueError('missing explicit Chair launch identities/prerequisite receipt')
    for key in ['call_sha', 'runtime_main_sha', 'budget_runtime_sha', 'isolated_dev_sha']:
        if not re.fullmatch('[a-f0-9]{40}', chair[key]):
            raise ValueError(f'not an exact prerequisite SHA: {key}')
        subprocess.run(['git', 'merge-base', '--is-ancestor', chair[key], 'HEAD'], cwd=ROOT, check=True)
    return chair


def prepare(out):
    sha = clean_sha()
    out.mkdir(parents=True, exist_ok=True)
    config(out, identities=False)
    if out.is_relative_to(ROOT):
        raise ValueError('proof area must be outside the source checkout')
    with socket.socket() as listener:
        listener.bind(('127.0.0.1', PORT))
    receipt = {'source_sha': sha, 'instance': INSTANCE, 'api_port': PORT, 'port_was_free': True,
               'checked_at': time.time(), 'nonce': uuid.uuid4().hex,
               'command': 'CODETERM_DEV_INSTANCE=blueprint-v2-proof CODETERM_DEV_API_PORT=17693 ./build.sh dev --wait'}
    with (out / 'prepared.json').open('x') as stream:
        json.dump(receipt, stream, indent=2)
    print('PREPARED. Chair runs the recorded launch command and saves its full output as launch.log; no process launched.')


class Bridge:
    def __init__(self, out, tab, mode='owner', label='owner'):
        self.context = out / 'contexts' / f'{label}.json'
        save(self.context, {'tab': tab, 'mode': mode, 'receipts': str(out / 'cli' / label)})
        self.out, self.label = out, label

    def call(self, *argv):
        env = dict(os.environ, CODETERM_DEV_INSTANCE=INSTANCE, CODETERM_DEV_API_PORT=str(PORT))
        result = subprocess.run(['bash', str(HERE / 'transport.sh'), '--context', str(self.context), '--', *map(str, argv)],
                                env=env, text=True, capture_output=True, timeout=90)
        if result.returncode:
            raise ValueError(f'CLI refused {argv[:2]} exit={result.returncode}: {redact(result.stderr)}')
        return json.loads(result.stdout)

    def api(self, path):
        return self.call('--api-get', path)

    def proxy(self, directory):
        directory.mkdir(parents=True, exist_ok=True)
        script = '#!/usr/bin/env bash\nexec bash ' + shlex.quote(str(HERE / 'transport.sh'))
        script += ' --context ' + shlex.quote(str(self.context)) + ' -- "$@"\n'
        for name in ['domios', 'codeterm']:
            path = directory / name
            path.write_text(script)
            path.chmod(0o700)
        return directory


class Events:
    def __init__(self, out, mem_dir, effects):
        self.queue = queue.Queue()
        self.log = (out / 'events.jsonl').open('x')
        self.process = subprocess.Popen(['node', str(HERE / 'watch.mjs'), f'http://127.0.0.1:{PORT}', str(mem_dir), str(effects)],
                                        stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        def read():
            for line in self.process.stdout:
                self.log.write(redact(line))
                self.log.flush()
                try:
                    self.queue.put(json.loads(line))
                except ValueError:
                    self.queue.put({'kind': 'failure', 'reason': 'invalid event stream'})
            self.queue.put({'kind': 'failure', 'reason': 'event process ended'})
        self.reader = threading.Thread(target=read, daemon=True)
        self.reader.start()
        if self.next(30).get('kind') != 'ready':
            raise ValueError('event subscriptions were not established before mutation')

    def next(self, timeout):
        try:
            event = self.queue.get(timeout=max(0.001, timeout))
        except queue.Empty as error:
            raise ValueError('bounded event deadline expired') from error
        if event.get('kind') == 'failure':
            raise ValueError(event['reason'])
        return event

    def condition(self, inspect, ready, seconds=180):
        deadline = time.monotonic() + seconds
        for _ in range(128):
            if time.monotonic() >= deadline:
                raise ValueError('bounded condition readiness expired')
            value = inspect()
            if time.monotonic() >= deadline:
                raise ValueError('bounded condition readiness expired')
            if ready(value):
                return value
            self.next(deadline - time.monotonic())
            if time.monotonic() >= deadline:
                raise ValueError('bounded condition readiness expired')
        raise ValueError('condition exceeded the registered event/read bound')

    def close(self):
        self.process.send_signal(signal.SIGTERM)
        try:
            self.process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            self.process.kill()
            self.process.wait(timeout=5)
        self.reader.join(timeout=5)
        stderr = self.process.stderr.read()
        self.log.close()
        Path(self.log.name).with_suffix('.stderr').write_text(redact(stderr))


def health_proof(bridge, out, sha, prepared):
    health = bridge.api('/api/health')
    native = bridge.api('/api/app/quit/snapshot')
    save(out / 'health.json', health)
    save(out / 'native.json', native)
    if health.get('status') != 'ok' or health.get('build_sha') != sha:
        raise ValueError('health/source build mismatch')
    if health.get('instance', {}).get('name') != INSTANCE or str(health['instance'].get('api_port')) != str(PORT):
        raise ValueError('health names the wrong DEV instance/port')
    if native.get('ready', {}).get('build') != sha or not native.get('client'):
        raise ValueError('exact tested build has no authenticated native attachment')
    ready, client = native['ready'], native['client']
    if ready.get('api_address') not in [f'127.0.0.1:{PORT}', f'[::1]:{PORT}']:
        raise ValueError('native readiness names a different API listener')
    events = []
    for line in (out / 'launch.log').read_text().splitlines():
        if line.startswith('DOMIOS_DEV_ATTACHMENT '):
            events.append(json.loads(line[len('DOMIOS_DEV_ATTACHMENT '):]))
    if not events or events[-1].get('epoch') != ready.get('epoch'):
        raise ValueError('launch lacks the current authenticated native readiness event')
    event = events[-1]
    if event.get('server_pid') != ready['process']['pid'] or event.get('client_pid') != client['process']['pid']:
        raise ValueError('native launch/client/server process identity mismatch')
    if client['connection'].get('server_epoch') != ready.get('epoch'):
        raise ValueError('native client is attached to a stale epoch')
    for pid in [event['server_pid'], event['client_pid']]:
        os.kill(pid, 0)
    if not prepared.get('port_was_free') or prepared.get('source_sha') != sha:
        raise ValueError('missing same-SHA free-port setup proof')
    return health


def actor_proof(bridge, tab, chair, out):
    value = bridge.call('tab', 'inspect', tab, '--json')
    save(out / 'actors' / f'{tab}.json', value)
    matches = [item for item in walk(value) if isinstance(item, dict)
               and item.get('id', item.get('tab_id')) == tab and item.get('team_id') == chair['team_id']]
    if not matches or any(item.get('workspace_id') not in [None, chair['workspace_id']] for item in matches):
        raise ValueError('CLI actor is not in the explicitly isolated DEV Team/workspace')
    if not any(Path(item.get('cwd') or '/').resolve().is_relative_to(out) for item in matches):
        raise ValueError('CLI actor cwd is outside the isolated proof area')


def sink_command(out, name):
    return ' '.join(shlex.quote(str(part)) for part in [sys.executable, '-I', out / 'sink.py', out / 'effects' / f'{name}.txt'])


def callable_fixture():
    return {'schema_version': 2, 'kind': 'pipeline', 'name': 'opaque-calibration-lookup',
            'note': 'Translate an opaque integer calibration code using the maintained calibration lookup.',
            'intent': 'translate a calibration code with the stored opaque calibration lookup',
            'inputs': {'code': {'type': 'integer', 'description': 'Opaque calibration code'}},
            'steps': [{'id': 'lookup', 'does': 'Translate one opaque calibration code with the stored lookup',
                       'code': {'lang': 'python', 'src': 'def main(value):\n    return {7: 73}[value]\n'},
                       'in': {'type': 'integer'}, 'out': {'type': 'integer'}}],
            'result': {'type': {'type': 'integer'}, 'from': 'lookup.out'}}


def pipeline_fixture(body, sink):
    return {**body, 'name': 'calibration-establishment', 'inputs': {},
            'trigger': {'type': 'input', 'kind': {'lit': 'text'}, 'source': {'lit': {'text': '7'}}},
            'steps': [*body['steps'], {'id': 'fixed-sink', 'run': {'lit': sink}, 'in': {'type': 'integer'}, 'bind': {'in': 'lookup.out'}}],
            'result': {'to': 'notify'}}


def completed(bridge, graph_id):
    log = bridge.call('blueprint', 'log', graph_id, '--json')
    entries = log if isinstance(log, list) else log.get('entries', [])
    return any(e.get('step_name') == '$run' and e.get('state') in ['done', 'failed'] for e in entries)


def execute(bridge, events, graph, out, name, expected, selected=None):
    applied = bridge.call('blueprint', 'apply', graph, '--json')
    save(out / name / 'apply.json', applied)
    graph_id = applied.get('id') or applied.get('blueprint', {}).get('id')
    if not graph_id:
        raise ValueError('real apply returned no graph identity')
    events.condition(lambda: completed(bridge, graph_id), bool)
    show = bridge.call('blueprint', 'show', graph_id, '--json')
    log = bridge.call('blueprint', 'log', graph_id, '--json')
    save(out / name / 'show.json', show)
    save(out / name / 'run.json', log)
    verify_run(show, log, out / 'effects' / f'{name}.txt', expected, selected)
    return graph_id


def establish(bridge, events, out):
    private = out / 'private'
    private.mkdir(mode=0o700)
    body = callable_fixture()
    save(private / 'callable.json', body)
    save(private / 'establish.json', pipeline_fixture(body, sink_command(out, 'establish')))
    execute(bridge, events, private / 'establish.json', out, 'establish', 73)
    saved = bridge.call('blueprint', 'keep', '--file', private / 'callable.json', '--when', body['intent'], '--json')
    save(private / 'keep.json', saved)
    raw_id = saved.get('id')
    if not raw_id:
        raise ValueError('real keep returned no persisted solution id')
    expected = {'id': raw_id if raw_id.startswith('mem:') else 'mem:' + raw_id}
    def ready(value):
        return value.get('recall', {}).get('status') == 'ready' and any(
            c.get('id') == expected['id'] and c.get('enrichment_pending') is False for c in value.get('cards', []))
    recalled = events.condition(lambda: bridge.call('blueprint', 'new', '--intent', body['intent'], '--json'), ready)
    save(private / 'readiness-candidates.json', recalled)
    card = verify_cards(recalled, expected)
    if card['signature'] != {'inputs': {'code': {'type': 'integer'}}, 'result': {'type': 'integer'}}:
        raise ValueError('saved product solution did not preserve its exact typed signature')
    return card


def read_subject_cli(out, key):
    records = []
    for path in sorted((out / 'cli' / key).glob('*.json')):
        metadata = json.loads(path.read_text())
        metadata['stdout'] = path.with_suffix('.stdout').read_text()
        records.append(metadata)
    blueprint = [r for r in records if r['argv'] and r['argv'][0] == 'blueprint']
    if not blueprint or blueprint[0]['argv'][:2] != ['blueprint', 'new'] or '--intent' not in blueprint[0]['argv']:
        raise ValueError('subject did not run new --intent first')
    if any(r['exit'] != 0 for r in records):
        raise ValueError('subject CLI refusal remains in its single bounded session')
    return records, json.loads(blueprint[0]['stdout'])


def run_case(key, provider, chair, bridge, events, out, selected, shipped):
    case = out / key
    cwd = case / 'work'
    cwd.mkdir(parents=True)
    proxy_bridge = Bridge(out, chair[f'{key}_tab'], 'subject', key)
    proxy = proxy_bridge.proxy(case / 'bin')
    binaries = [chair['codex_binary'], chair['claude_binary']]
    env = subject_environment(proxy, binaries)
    identity = subprocess.run([str(proxy / 'domios'), 'identify', '--json'], cwd=cwd, env=env, text=True, capture_output=True, timeout=90)
    save(case / 'scrubbed-identify.json', {'exit': identity.returncode, 'stdout': identity.stdout, 'stderr': identity.stderr,
                                         'env_keys': sorted(env)})
    if identity.returncode:
        raise ValueError('domios identify failed in the scrubbed subject environment; no author launched')
    verify_identity(json.loads(identity.stdout))
    task = json.loads((HERE / 'tasks.json').read_text())['positive' if selected else 'no_fit'].format(
        name=f'proof-{key}', sink=sink_command(out, key))
    prompt = shipped + '\n\nTask\n\n' + task + '\n'
    if selected and re.search(r'(?<![.\d])73(?!\d)', prompt):
        raise ValueError('positive initial context contains the hidden result')
    session = launch(provider, chair[f'{provider}_binary'], 'gpt-6.1-sol' if provider == 'codex' else chair['claude_model'],
                     prompt, cwd, proxy, binaries, case)
    records, candidates = read_subject_cli(out, key)
    save(case / 'actual-candidates.json', candidates)
    verify_cards(candidates, selected)
    checked = json.loads((cwd / 'checked.json').read_text())
    authored = json.loads((cwd / 'authored.json').read_text())
    sealed = json.loads((cwd / 'sealed.json').read_text())
    if sealed != checked_definition(checked):
        raise ValueError('sealed graph differs from the actual checked definition')
    checks = [json.loads(record['stdout']) for record in records if record['argv'][:2] == ['blueprint', 'check']]
    if checked not in checks:
        raise ValueError('checked.json was not the actual CLI check response')
    if not authored.get('steps'):
        raise ValueError('missing authored graph')
    verify_authored(sealed, f'proof-{key}', selected, sink_command(out, key))
    expected = 73 if selected else 11
    graph_id = execute(bridge, events, cwd / 'sealed.json', out, key, expected, selected)
    return {'case': key, 'status': 'PASS', 'session_id': session['data']['session_id'], 'graph_id': graph_id,
            'prompt_sha256': digest(prompt), 'effect': expected, 'usage': session['data']['usage']}


def live(out):
    sha = clean_sha()
    chair = config(out)
    prepared = json.loads((out / 'prepared.json').read_text())
    with (out / 'live-started.json').open('x') as stream:
        json.dump({'sha': sha, 'started_at': time.time(), 'owner': 'Chair'}, stream)
    bridge = Bridge(out, chair['orchestrator_tab'])
    health_proof(bridge, out, sha, prepared)
    for tab in [chair[key] for key in ['orchestrator_tab', 'codex_tab', 'claude_tab', 'no_fit_tab']]:
        actor_proof(bridge, tab, chair, out)
    effects = out / 'effects'
    effects.mkdir()
    (out / 'sink.py').write_bytes((HERE / 'sink.py').read_bytes())
    shipped = (ROOT / 'prod-docs/BLUEPRINT-AUTHORING.md').read_text()
    decision = json.loads((HERE / 'prompt-decision.json').read_text())
    if digest(shipped) != decision['product_sha256']:
        raise ValueError('shipped prompt bytes differ from the accepted product snapshot')
    delivered = bridge.call('docs', 'BLUEPRINT-AUTHORING', '--json')
    if delivered.get('content') != shipped:
        raise ValueError('QA CLI does not ship the exact source prompt bytes')
    save(out / 'shipped-prompt.json', delivered)
    for provider in ['codex', 'claude']:
        binary = Path(chair[f'{provider}_binary'])
        if not binary.is_absolute() or not os.access(binary, os.X_OK):
            raise ValueError(f'missing supported {provider} binary')
        verify_help(provider, str(binary), subject_environment(out, [str(binary)]), out)
    mem = Path(os.environ['CODETERM_DATA_DIR']) / 'mem'
    if not mem.is_dir():
        raise ValueError('selected named DEV has no memory directory to observe')
    events = Events(out, mem, effects)
    outcomes = []
    try:
        selected = establish(bridge, events, out)
        for key, provider, card in [('codex', 'codex', selected), ('claude', 'claude', selected), ('no_fit', 'codex', None)]:
            try:
                outcome = run_case(key, provider, chair, bridge, events, out, card, shipped)
            except Exception as error:
                outcome = {'case': key, 'status': 'FAIL', 'reason': str(error)}
            outcomes.append(outcome)
            save(out / key / 'outcome.json', outcome)
    finally:
        events.close()
    distinct = len({o.get('session_id') for o in outcomes if o.get('session_id')}) == 3
    if clean_sha() != sha:
        raise ValueError('source changed during proof')
    passed = distinct and len(outcomes) == 3 and all(o['status'] == 'PASS' for o in outcomes)
    result = {'status': 'PASS' if passed else 'FAIL', 'sha': sha, 'clean_tip': True, 'instance': INSTANCE,
              'port': PORT, 'prompt_sha256': digest(shipped), 'authors': outcomes,
              'native_platform': platform.system(),
              'unverified_platforms': [name for name in ['Darwin', 'Windows', 'Linux'] if name != platform.system()],
              'simulation': False}
    save(out / 'result.json', result)
    if not passed:
        raise ValueError('one or more required real author/check/execution/effect cases failed')
    print(json.dumps(result, indent=2))


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--prepare-launch', action='store_true')
    args = parser.parse_args()
    out = args.out.expanduser().resolve()
    if os.environ.get('CODETERM_INSTANCE') != INSTANCE or os.environ.get('CODETERM_API_PORT') != str(PORT):
        raise ValueError('wrong named DEV identity')
    try:
        prepare(out) if args.prepare_launch else live(out)
    except Exception as error:
        save(out / 'failure.json', {'status': 'FAIL', 'reason': str(error), 'simulation': False})
        print(f'FAIL: {error}', file=sys.stderr)
        raise SystemExit(1)


if __name__ == '__main__':
    main()
