"""Codex and OpenCode live matrix; all CLI operations go through the DEV-pinned dc."""
import concurrent.futures
import json
import os
import pathlib
import shlex
import subprocess
import sys
import time
import uuid

import codex_oracle as oracle
import observe

ROOT = pathlib.Path(os.environ["RL_BENCH_ROOT"])
PREBUILT = pathlib.Path(os.environ["RL_PREBUILT_CWD"])
PAD = pathlib.Path(os.environ["RL_PAD_DIR"])
MODE = sys.argv[1]
OUT = pathlib.Path(os.environ["RL_RUN_DIR"]) / MODE
OUT.mkdir(parents=True, exist_ok=True)
DEADLINE = int(os.environ.get("RL_BIND_DEADLINE", "180"))
TURN_DEADLINE = int(os.environ.get("RL_TURN_DEADLINE", "240"))
RUN = uuid.uuid4().hex[:10]


class Matrix:
    def __init__(self):
        self.workspace = None
        self.team = None
        self.plan = None
        self.orch = None
        self.tabs = []
        self.workers = []
        self.tasks = {}
        self.results = []
        self.phase = 1
        self.serial = 0
        self.log_path = pathlib.Path(os.environ["CODETERM_DEV_DATA_DIR"]) / "logs/codeterm.log"
        self.log_offset = self.log_path.stat().st_size if self.log_path.exists() else 0

    def write(self, label, value):
        path = OUT / label
        path.write_text(value if isinstance(value, str) else json.dumps(value, indent=2) + "\n", encoding="utf-8")
        return path

    def cli(self, label, *args, caller=None, body=None, json_output=True):
        command = ['bash', '-c', 'source "$1/evals/_lib/lib.sh"; shift; dc "$@"', 'restore-bench', str(ROOT)]
        if caller:
            command += ['--as-tab', caller]
        command += list(args)
        if json_output:
            command += ['--json']
        source = None
        if body is not None:
            source = PAD / f"rl-{RUN}-{label}.md"
            source.write_text(body, encoding="utf-8")
        result = subprocess.run(command, input=source.read_text() if source else None,
                                text=True, capture_output=True, timeout=TURN_DEADLINE)
        token = os.environ.get('CODETERM_API_TOKEN', '')
        stdout = result.stdout.replace(token, '[redacted]') if token else result.stdout
        stderr = result.stderr.replace(token, '[redacted]') if token else result.stderr
        self.write(label + '.stdout', stdout)
        self.write(label + '.stderr', stderr)
        self.write(label + '.exit', str(result.returncode))
        oracle.require(result.returncode == 0, f"{label} exit={result.returncode}\n{stdout}\n{stderr}")
        if json_output:
            value = json.loads(result.stdout)
            self.write(label + '.json', value)
            return value
        return stdout

    def inspect(self, tab):
        self.serial += 1
        return oracle.inspected(self.cli(f'inspect-{self.serial}-{tab}', 'tab', 'inspect', tab, '--lifecycle'))

    def snapshot(self, label):
        records = {tab: self.inspect(tab) for tab in self.tabs}
        self.write(label + '.json', records)
        return records

    def wait(self, predicate, label):
        return observe.EventWait(DEADLINE).until(lambda _: predicate(), label)

    def send(self, tab, body, label):
        return self.cli(label, 'send', '--tab', tab, '--from-pad', '-', '--require-agent',
                        caller=self.orch, body=body)

    def queue_send(self, tab, marker, label):
        path = PAD / f'rl-{RUN}-{label}.md'
        path.write_text(f'Record this marker once: {marker}. Continue the original work.')
        receipt = observe.api('POST', f'/api/tabs/{tab}/chat-send', {'text': path.read_text(), 'correlationId': marker})
        self.write(label + '.json', receipt)
        oracle.require(receipt.get('state') == 'held', f'{tab}: queue admission was not held: {receipt}')
        return receipt

    def long_turn(self, tab, label):
        path = PAD / f'rl-{RUN}-{label}-{tab}.md'
        path.write_text('Write integers from 1 through 3000 one per line without tools. If interrupted, resume where you left off. Do not finish until all are written.')
        observe.cmd_send_long_turn(TURN_DEADLINE, tab, path.read_text())

    def session_file(self, tab):
        row = self.inspect(tab)
        data = self.cli(f'session-{self.serial}-{tab}', 'sessions', 'get', row['agent_session_id'])
        path = data.get('transcript_path') or data.get('file_path')
        oracle.require(path and pathlib.Path(path).is_file(), f'{tab}: own-session transcript unavailable: {data}')
        return pathlib.Path(path)

    def health(self):
        health = observe.api('GET', '/api/health')
        self.write('health.json', health)
        sha = health.get('build', '')
        oracle.require(sha and os.environ['RL_EXPECTED_SHA'].startswith(sha), f"health {sha} differs from approved SHA")
        oracle.require(health.get('instance', {}).get('name') == 'restartqa', f"wrong health instance: {health}")

    def restart(self, label, queued=None):
        observe.api('POST', '/api/session/save')
        since = time.time()
        before_size = self.log_path.stat().st_size
        env = dict(os.environ)
        env['CODETERM_INSTANCE'], env['CODETERM_API_PORT'] = 'restartqa', '7692'
        if MODE == 'gui':
            env['RL_GUI_MODE'] = '1'
            env['NODE_OPTIONS'] = '--require=' + str(ROOT / 'evals/restore-lifecycle/gui-env.cjs')
        else:
            env.pop('RL_GUI_MODE', None)
            env.pop('NODE_OPTIONS', None)
        with (OUT / (label + '-restart.log')).open('w') as log:
            stopped = subprocess.run(['bash', 'build.sh', 'kill'], cwd=PREBUILT, env=env, stdout=log, stderr=subprocess.STDOUT, timeout=60)
            oracle.require(stopped.returncode == 0, f'{label}: build.sh kill failed')
            launch = subprocess.Popen(['bash', 'build.sh', 'dev-prebuilt', '--wait'], cwd=PREBUILT, env=env, stdout=log, stderr=subprocess.STDOUT)
            # build.sh's readiness receipt is an event; the daemon may still be restoring tabs.
            oracle.require(launch.wait(timeout=120) == 0, f'{label}: dev-prebuilt failed')
        self.health()
        self.environment(label)
        if queued:
            states = {tab: self.inspect(tab) for tab in queued}
            self.write(label + '-queue-window.json', states)
            oracle.require(all(row.get('boot_stage') != 'agent_ready' for row in states.values()), f'{label}: queue window was missed')
            with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
                sends = [pool.submit(self.queue_send, tab, marker, label + '-queue-' + tab) for tab, marker in queued.items()]
                for sent in sends:
                    sent.result()
        if self.tabs:
            observe.cmd_wait_bound(DEADLINE, self.tabs)
        from restore_report import assert_settled
        assert_settled([str(DEADLINE), str(since), *self.tabs], observe.EventWait, observe.fail)
        if self.tabs:
            self.wait(lambda: all(self.inspect(t).get('boot_stage') == 'agent_ready' for t in self.tabs), label + ' agent_ready')
        return before_size

    def environment(self, label):
        result = subprocess.run(['lsof', '-t', '-iTCP:7692', '-sTCP:LISTEN'], capture_output=True, text=True, check=True)
        pids = result.stdout.split()
        oracle.require(len(pids) == 1, f'expected one restartqa daemon, got {pids}')
        result = subprocess.run(['ps', 'eww', '-p', pids[0], '-o', 'command='], capture_output=True, text=True, check=True)
        # Store only relevant environment entries, never tokens from ps output.
        entries = [part for part in shlex.split(result.stdout) if part.startswith(('PATH=', 'HOME=', 'USER=', 'CODETERM_INSTANCE=', 'CODETERM_API_PORT='))]
        raw = '\n'.join(entries)
        self.write(label + '-daemon-env.txt', raw)
        oracle.daemon_environment(raw, MODE)

    def log_since(self, offset, label):
        with self.log_path.open('rb') as stream:
            if self.log_path.stat().st_size < offset:
                offset = 0
            stream.seek(offset)
            raw = stream.read().decode('utf-8', errors='replace')
        self.write(label + '-dev.log', raw)
        return raw

    def pass_phase(self, phase, detail):
        self.results.append((phase, 'PASS', detail))
        print(f'PASS mode={MODE} phase={phase} {detail} evidence={OUT}', flush=True)
        self.table()

    def table(self):
        lines = ['| Mode | Phase | Result | Evidence |', '|---|---|---|---|']
        for phase, verdict, detail in self.results:
            lines.append(f'| {MODE} | {phase} | {verdict} | {detail.replace(chr(10), " ")} ({OUT}) |')
        self.write('results.md', '\n'.join(lines) + '\n')

    def phase_environment(self):
        self.health()
        continuation = self.cli('continue-config', 'config', 'get', 'agent.continue_after_restart')
        oracle.require(continuation.get('value') is True, f'continue_after_restart must be enabled: {continuation}')
        oracle.require(subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=PREBUILT, text=True).strip() == os.environ['RL_EXPECTED_SHA'], 'canonical checkout does not match approved SHA')
        # Starting each mode is itself a scoped full stop/start, before fixture creation.
        self.restart('mode-start')
        info = self.cli('cli-status', 'agent', 'cli', 'status', 'codex')
        oracle.require(info.get('detected_version') and info.get('version_probe') == 'detected', f'Codex version not detected: {info}')
        binary = info.get('resolved_path')
        oracle.require(binary, f'Codex resolved binary missing: {info}')
        executable = pathlib.Path(binary).resolve()
        with executable.open('rb') as stream:
            shebang = stream.read(128).split(b'\n', 1)[0]
        self.write('codex-entrypoint.json', {'entrypoint': binary, 'resolved': str(executable), 'shebang': shebang.decode(errors='replace')})
        oracle.require(shebang.startswith(b'#!/usr/bin/env node'), f'installed Codex is not the required env-node wrapper: {binary}')
        if MODE == 'gui':
            probe = subprocess.run([str(executable), '--version'], env={'PATH': oracle.GUI_PATH, 'HOME': os.environ['HOME'], 'USER': os.environ['USER']}, capture_output=True, text=True)
            self.write('wrapper-minimal-path.txt', f'exit={probe.returncode}\n{probe.stdout}{probe.stderr}')
            oracle.require(probe.returncode == 127 and 'node' in probe.stderr, 'GUI fixture did not exercise the missing env-node dependency')
        self.pass_phase(1, 'daemon PATH and npm env-node wrapper proven; version detected')

    def phase_spawn(self):
        ws = self.cli('workspace-create', 'workspace', 'create', f'Restore{RUN}', '--provision')
        self.workspace = ws['id']
        pair = self.cli('team-pair', 'team', 'pair', '--orchestrator', 'codex', '--planner', 'codex', '--workspace', self.workspace, '--title', f'Restore{RUN}')
        self.orch = pair['orchestrator_tab_id']
        self.tabs = [self.orch, pair['planner_tab_id']]
        observe.cmd_wait_bound(DEADLINE, self.tabs)
        self.team = self.inspect(self.orch)['team_id']
        model = os.environ.get('RL_CODEX_MODEL')
        if not model:
            catalog = self.cli('models', 'agent', 'models', 'codex')
            cheap = [row['id'] for row in catalog if 'affordable' in row.get('description', '').lower()]
            oracle.require(cheap, 'set RL_CODEX_MODEL: catalogue has no affordable model')
            model = cheap[0]
        plan = self.cli('plan-create', 'plan', 'create', '--title', f'Restore{RUN}', '--project', ws['cwd'], caller=self.orch)
        self.plan = plan.get('plan_id') or plan.get('id')
        oracle.require(self.plan, f'plan id absent: {plan}')
        self.cli('plan-goal', 'plan', 'goal', 'set', self.plan, '--vector', 'Codex restore evidence', caller=self.orch)
        self.cli('plan-approve', 'plan', 'mode', self.plan, 'approved', caller=self.orch)
        self.cli('plan-mode', 'plan', 'mode', self.plan, 'execution', caller=self.orch)
        for index, title in enumerate(('Alpha', 'Beta', 'Gamma')):
            body = 'Run codeterm agent ack immediately. This is a read-only restore probe. Do not edit files, spawn agents, report completion, or change plan state. Reply READY and remain idle until instructed. Follow later bench messages exactly.'
            body_path = PAD / f'rl-{RUN}-{title}.md'
            body_path.write_text(body)
            task = self.cli('task-add-' + title, 'plan', 'task', 'add', self.plan, '--name', title,
                            '--executor-role', 'Worker', '--personality', title, '--role-guidance-file', str(body_path),
                            '--body-file', str(body_path), '--criterion', 'Exact session survives restart',
                            '--test-plan', 'Read-only live restore probe', '--correctness', 'No repository mutation',
                            '--execution-intent', 'non_repository', caller=self.orch)
            task_id = task.get('task_id') or task.get('id') or task.get('task', {}).get('id')
            oracle.require(task_id, f'task id absent: {task}')
            if index < 2:
                spawn = self.cli('dispatch-' + title, 'plan', 'task', 'dispatch', self.plan, task_id,
                                 '--agent', 'codex', '--model', model, '--execution-intent', 'read_only',
                                 '--workspace', self.workspace, caller=self.orch)
            else:
                spawn = self.cli('spawn-' + title, 'agent', 'spawn', 'codex', '--role', 'Worker',
                                 '--plan', self.plan, '--plan-task', task_id, '--parent-tab', self.orch,
                                 '--workspace', self.workspace, '--model', model, '--execution-intent', 'read_only',
                                 '--title', title, '--description', 'Read-only Codex restore probe', '--non-user-facing', caller=self.orch)
            tab = spawn['tab_id']
            self.tabs.append(tab)
            self.workers.append(tab)
            self.tasks[tab] = task_id
        observe.cmd_wait_bound(DEADLINE, self.tabs)
        self.wait(lambda: all(self.inspect(t).get('boot_stage') == 'agent_ready' for t in self.tabs), 'spawn agent_ready')
        baseline = self.snapshot('spawn')
        oracle.bindings(baseline)
        oracle.require(all(row.get('cwd') == ws['cwd'] for row in baseline.values()), 'spawn matrix agents do not share one cwd')
        self.wait(lambda: all(observe.session_owner(row['agent_session_id'])[0] == 'codex' for row in baseline.values()), 'provider store owns all five exact sessions')
        for tab in self.workers:
            self.wait(lambda tab=tab: self.acked(tab), tab + ' task ACK')
        for tab in self.tabs:
            path = self.session_file(tab)
            self.write('transcript-' + tab + '.path', str(path))
            texts = oracle.user_texts(path)
            oracle.require(texts, f'{tab}: own-session transcript has no user turn')
            self.wait(lambda path=path: any('startup_acknowledged' in text or 'ACKNOWLEDGED' in text for text in oracle.tool_outputs(path)), tab + ' own startup ACK result')
        # This UI proof is intentionally mandatory and never inferred from an HTTP 200.
        self.cli('browser-open', 'plugin', 'browser', 'open', 'http://127.0.0.1:7692/#token=' + os.environ['CODETERM_API_TOKEN'], caller=self.orch, json_output=False)
        self.cli('browser-ready', 'plugin', 'browser', 'wait', '--selector', '[data-testid=sidebar]', caller=self.orch, json_output=False)
        for tab, row in baseline.items():
            self.cli('focus-' + tab, 'tab', 'focus', tab, json_output=False)
            self.cli('browser-select-' + tab, 'plugin', 'browser', 'click', 'button', 'Open tab ' + row['title'], caller=self.orch, json_output=False)
            view = self.cli('browser-chat-' + tab, 'plugin', 'browser', 'snapshot', caller=self.orch, json_output=False)
            oracle.require('Starting' not in view, f'{tab}: Chat still Starting')
            oracle.require(row['title'] in view and ('READY' in view or 'acknowledged' in view.lower()), f'{tab}: Chat transcript not visibly initialized')
        self.cli('browser-close', 'plugin', 'browser', 'close', caller=self.orch, json_output=False)
        self.pass_phase(2, 'five distinct nonce-bound agent_ready sessions, ACKs and visible Chat')

    def acked(self, tab):
        self.serial += 1
        detail = self.cli(f'task-ack-{self.serial}', 'task', 'show', self.tasks[tab])
        try:
            oracle.current_attempt(detail, tab)
            return True
        except AssertionError:
            return False

    def phase_cycles(self):
        self.phase = 3
        for cycle in range(1, 4):
            label = f'cycle-{cycle}'
            paths = {tab: self.session_file(tab) for tab in self.tabs}
            before = self.snapshot(label + '-before')
            before_texts = {tab: oracle.user_texts(path) for tab, path in paths.items()}
            baseline = {tab: len(texts) for tab, texts in before_texts.items()}
            with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
                tasks = [pool.submit(self.long_turn, tab, label) for tab in self.workers[:2]]
                for task in tasks:
                    task.result()
            working = {tab: observe.status(tab) or {} for tab in self.workers[:2]}
            self.write(label + '-working.json', working)
            oracle.require(all(row.get('status', '').lower() == 'working' for row in working.values()), 'both Workers must be Working at restart')
            markers = {tab: f'RL-QUEUE-{RUN}-{cycle}-{tab}' for tab in self.workers[:2]}
            self.phase = 4
            offset = self.restart(label, markers)
            self.phase = 3
            self.wait(lambda: all(any(oracle.CONTINUE in text for text in oracle.user_texts(paths[t])[baseline[t]:]) for t in self.workers[:2]), label + ' owed continue in own transcripts')
            self.wait(lambda: all(any(marker in text for text in oracle.user_texts(paths[t])) for t, marker in markers.items()), label + ' queued marker delivery')
            self.wait(lambda: all(oracle.resumed_numeric_output(paths[t], baseline[t]) for t in self.workers[:2]), label + ' unprompted resumed output')
            after = self.snapshot(label + '-after')
            oracle.bindings(after, before)
            log = self.log_since(offset, label)
            oracle.restart_logs(log, self.workers[:2])
            for tab in self.workers[:2]:
                delta = oracle.user_texts(paths[tab])[baseline[tab]:]
                oracle.require(sum(oracle.CONTINUE in text for text in delta) == 1, f'{tab}: continue was not exactly once')
            transcripts = {t: oracle.user_texts(p) for t, p in paths.items()}
            for tab, texts in before_texts.items():
                oracle.require(transcripts[tab][:len(texts)] == texts, f'{tab}: original transcript lost a user turn')
            oracle.exact_markers(transcripts, markers)
            self.write(label + '-transcript-evidence.json', {tab: {'session': before[tab]['agent_session_id'], 'path': str(paths[tab]), 'baseline_users': baseline[tab], 'users_after': len(transcripts[tab]), 'continues_added': sum(oracle.CONTINUE in text for text in transcripts[tab][baseline[tab]:]), 'markers': {owner: sum(marker in text for text in transcripts[tab]) for owner, marker in markers.items()}, 'resumed_numeric_output': oracle.resumed_numeric_output(paths[tab], baseline[tab])} for tab in self.tabs})
            reconcile = self.cli(label + '-reconcile', 'plan', 'reconcile', self.plan, caller=self.orch)
            oracle.require('in_progress_tab_gone' not in json.dumps(reconcile.get('drift', [])), f'{label}: {reconcile}')
            self.pass_phase(3, f'{label}: same sessions, one continue each, original work resumed, reconcile clean')
            self.pass_phase(4, f'{label}: each queued marker exactly once, no cross-delivery')
            self.idle()

    def idle(self):
        self.wait(lambda: all((observe.status(t) or {}).get('status', '').lower() == 'idle' for t in self.tabs), 'all agents Idle')

    def phase_respawn(self):
        self.phase = 5
        old = self.workers[2]
        detail = self.cli('respawn-old-task', 'task', 'show', self.tasks[old])
        attempt = oracle.current_attempt(detail, old)
        new = self.cli('respawn', 'plan', 'task', 'respawn', self.plan, self.tasks[old], '--attempt', attempt['id'], caller=self.orch)
        tab = new['tab_id']
        self.tabs.append(tab)
        self.tasks[tab] = self.tasks[old]
        observe.cmd_wait_bound(DEADLINE, [tab])
        self.wait(lambda: self.acked(tab), 'respawned Worker ACK')
        self.cli('old-close', 'tab', 'close', old, caller=self.orch)
        self.tabs.remove(old)
        detail = self.cli('respawn-after-close', 'task', 'show', self.tasks[tab])
        oracle.current_attempt(detail, tab)
        marker = 'RL-REPORT-' + RUN
        self.send(tab, f'Write {marker} to your AgentPad report.md, then run codeterm agent report --status in_review --from-pad report.md. Do not modify any repository file. Reply with the accepted report result.', 'request-report')
        def accepted():
            detail = self.cli('report-task', 'task', 'show', self.tasks[tab])
            task = detail.get('task', detail)
            return task.get('status') == 'in_review' and marker in json.dumps(detail)
        self.wait(accepted, 'new Worker report accepted and persisted')
        log = self.log_since(self.log_offset, 'respawn')
        oracle.require('STALE TASK BINDING' not in log, 'STALE TASK BINDING in DEV log')
        self.workers[2] = tab
        self.pass_phase(5, 'new assignment survives old close; actual Worker report persisted')

    def phase_foreign(self):
        self.phase = 6
        tab = self.workers[0]
        before = self.inspect(tab)
        detail = self.cli('foreign-before-task', 'task', 'show', self.tasks[tab])
        attempt = oracle.current_attempt(detail, tab)
        marker = 'RL-FOREIGN-' + RUN
        helper = ROOT / 'evals/restore-lifecycle/foreign_codex.py'
        prompt = f'Run python3 {shlex.quote(str(helper))} {shlex.quote(marker)} in your own tool environment. It starts a scratch Codex inheriting your CODETERM_* variables and exits it after its first response. Do not clear those variables. Reply with the helper JSON output. Do not report task completion.'
        self.send(tab, prompt, 'foreign-request')
        path = self.session_file(tab)
        self.wait(lambda: any(marker in text and '\"foreign_exit\": 0' in text for text in oracle.tool_outputs(path)), 'foreign scratch Codex exit evidence')
        after = self.inspect(tab)
        oracle.bindings({tab: after}, {tab: before})
        oracle.require(before.get('session_binding_nonce') == after.get('session_binding_nonce'), 'foreign Codex rotated binding')
        detail = self.cli('foreign-after-task', 'task', 'show', self.tasks[tab])
        oracle.require(oracle.current_attempt(detail, tab)['id'] == attempt['id'], 'foreign Codex changed attempt')
        self.pass_phase(6, 'scratch Codex inherited Worker environment; binding/session/attempt unchanged')

    def phase_idle(self):
        self.phase = 7
        self.idle()
        before = self.snapshot('idle-before')
        paths = {t: self.session_file(t) for t in self.tabs}
        texts = {t: oracle.user_texts(p) for t, p in paths.items()}
        offset = self.restart('idle')
        self.idle()
        oracle.bindings(self.snapshot('idle-after'), before)
        oracle.restart_logs(self.log_since(offset, 'idle'), self.tabs, idle=True)
        for tab, path in paths.items():
            oracle.require(oracle.user_texts(path) == texts[tab], f'{tab}: idle restart sent or lost a user turn')
        self.pass_phase(7, 'all idle sessions preserved; no continue or queued turn sent')

    def run(self):
        try:
            self.phase_environment()
            self.phase = 2
            self.phase_spawn()
            self.idle()
            self.phase_cycles()
            self.phase_respawn()
            self.phase_foreign()
            self.phase_idle()
            from opencode_track import OpenCodeTrack
            OpenCodeTrack(self).run()
        except BaseException as error:
            self.write('failure.txt', str(error))
            if self.log_path.exists():
                self.log_since(self.log_offset, 'failure')
            self.results.append((self.phase, 'FAIL', str(error)))
            executed = {row[0] for row in self.results}
            for phase in range(1, 12):
                if phase not in executed:
                    self.results.append((phase, 'FAIL', 'NOT EXECUTED: prerequisite phase failed'))
            self.table()
            print(f'FAIL mode={MODE} phase={self.phase} {error} evidence={OUT}', flush=True)
            raise
        finally:
            # Preserve failed agent fixtures but close every browser utility we opened.
            if self.orch:
                try:
                    self.cli('browser-cleanup', 'plugin', 'browser', 'close', caller=self.orch, json_output=False)
                except Exception:
                    pass
        if self.team:
            self.cli('team-cleanup', 'team', 'close', self.team)
        if self.workspace:
            self.cli('workspace-cleanup', 'workspace', 'close', self.workspace, '--force', json_output=False)
        if self.plan:
            self.cli('plan-cleanup', 'plan', 'delete', self.plan, json_output=False)


if __name__ == '__main__':
    oracle.require(MODE in ('gui', 'normal'), 'mode must be gui or normal')
    Matrix().run()
