"""Serialized OpenCode spawn, live Chat and lazy-session restore track."""
import concurrent.futures
import os
import time

import codex_oracle as oracle
import observe
import opencode_evidence as evidence


class OpenCodeTrack:
    def __init__(self, host):
        self.host = host
        self.tabs = []
        self.expected = {}
        self.findings = []
        self.serial = 0
        self.browser_open = False

    def finding(self, tab, kind, detail, path):
        self.findings.append(dict(tab=tab, kind=kind, detail=detail, evidence=str(path)))
        self.host.write('opencode-findings.json', self.findings)

    def chat(self, tab, label):
        before = self.host.inspect(tab)
        payload = self.host.cli(label, 'tab', 'chat', '--tab', tab)
        rows = payload.get('messages', []) if isinstance(payload, dict) else payload
        after = self.host.inspect(tab)
        oracle.require(before.get('agent_session_id') and before.get('agent_session_id') == after.get('agent_session_id'), f'{tab}: Chat read crossed a session change')
        try:
            evidence.row_ids(rows)
        except AssertionError as error:
            self.finding(tab, 'transcript_rows', str(error), label + '.json')
            raise
        duplicates = evidence.repeated_text(rows)
        if duplicates:
            self.finding(tab, 'repeated_transcript_text', duplicates, label + '.json')
        return rows

    def chats(self, label):
        self.serial += 1
        return {tab: self.chat(tab, f'opencode-{label}-{self.serial}-{tab}') for tab in self.tabs}

    def browser(self, label, *args):
        return self.host.cli('opencode-browser-' + label, 'plugin', 'browser', *args,
                             caller=self.host.orch, json_output=False)

    def select(self, tab):
        row = self.host.inspect(tab)
        self.browser('select-' + tab, 'click', 'button', 'Open tab ' + row['title'])

    def visible(self, tab, marker, label):
        self.browser(label + '-wait', 'wait', '--text', marker)
        view = self.browser(label, 'snapshot')
        if 'Starting' in view:
            self.finding(tab, 'stuck_starting', 'Chat shows Starting after own-session reply', 'opencode-browser-' + label + '.stdout')
        oracle.require('Starting' not in view and marker in view, f'{tab}: visible Chat reply absent or still Starting')
        return view

    def startup_ack(self, tab):
        self.serial += 1
        rows = self.chat(tab, f'opencode-ack-{self.serial}-{tab}')
        return any('startup_acknowledged' in row.get('toolResult', '') or
                   'ACKNOWLEDGED' in row.get('toolResult', '') for row in rows if row.get('type') == 'tool_call')

    def spawn(self):
        host = self.host
        host.phase = 8
        catalog = host.cli('opencode-models', 'agent', 'models', 'opencode')
        cheap = os.environ.get('RL_OPENCODE_MODEL')
        if not cheap:
            candidates = [row['id'] for row in catalog if any(term in row.get('description', '').lower() for term in ('free', 'cheap', 'affordable'))]
            oracle.require(candidates, 'set RL_OPENCODE_MODEL: no cheap OpenCode catalogue entry')
            cheap = candidates[0]
        providers = host.cli('opencode-providers', 'agent', 'providers')
        providers = providers.get('providers', []) if isinstance(providers, dict) else providers
        provider = next(row for row in providers if row['id'] == 'opencode')
        default = provider.get('default_model') or provider.get('defaultModel')
        oracle.require(default, 'OpenCode default model is not advertised')
        oracle.require(cheap != default, 'cheap OpenCode model must differ from default')
        host.cli('opencode-cli-status', 'agent', 'cli', 'status', 'opencode')
        offset = host.log_path.stat().st_size
        for title, model, worker in (('Delta', default, True), ('Epsilon', cheap, True), ('Zeta', default, False)):
            startup = 'RL-OPENCODE-START-' + title
            prompt = ('Read-only probe. Do not edit files, spawn agents, report completion, or change plan state. '
                      'Run codeterm agent startup-ack immediately. ' +
                      ('Run codeterm agent ack immediately. ' if worker else '') +
                      f'Reply {startup} and remain idle until instructed. Follow later bench messages exactly.')
            path = host.write('opencode-start-' + title + '.md', prompt)
            # CLI task-file bodies must live in the caller's AgentPad.
            from codex_matrix import PAD, RUN
            body = PAD / f'rl-{RUN}-opencode-{title}.md'
            body.write_text(prompt)
            args = ['agent', 'spawn', 'opencode', '--workspace', host.workspace,
                    '--title', title, '--description', 'Read-only OpenCode spawn and lazy-session probe',
                    '--task-file', str(body), '--non-user-facing']
            if model == cheap:
                args += ['--model', model]
            if worker:
                task = host.cli('opencode-task-' + title, 'plan', 'task', 'add', host.plan,
                                '--name', title, '--executor-role', 'Worker', '--personality', title,
                                '--role-guidance-file', str(body), '--body-file', str(body),
                                '--criterion', 'Own lazy session survives restart', '--test-plan', 'Read-only restore probe',
                                '--correctness', 'No repository mutation', '--execution-intent', 'non_repository', caller=host.orch)
                task_id = task.get('task_id') or task.get('id') or task.get('task', {}).get('id')
                oracle.require(task_id, f'task id absent: {task}')
                args += ['--role', 'Worker', '--parent-tab', host.orch, '--plan', host.plan,
                         '--plan-task', task_id, '--execution-intent', 'read_only']
            else:
                args += ['--team', host.team]
            start_ms = int(time.time() * 1000)
            waiter = observe.EventWait(int(os.environ.get('RL_BIND_DEADLINE', '180')))
            spawned = host.cli('opencode-spawn-' + title, *args, caller=host.orch)
            tab = spawned['tab_id']
            self.tabs.append(tab)
            host.tabs.append(tab)
            self.expected[tab] = model
            if worker:
                host.tasks[tab] = task_id
            times = {}
            observations = []
            def ready(event):
                row = host.inspect(tab)
                elapsed = int(time.time() * 1000) - start_ms
                state = observe.status(tab) or {}
                observations.append(dict(elapsed_ms=elapsed, event=event, inspect=row, status=state))
                host.write('opencode-launch-observations-' + tab + '.json', observations)
                if state.get('status', '').lower() in ('waiting', 'needs_input', 'blocked'):
                    self.finding(tab, 'prompt_or_wait', state, 'opencode-launch-observations-' + tab + '.json')
                if row.get('agent_session_id'):
                    times.setdefault('session_binding_observed_ms', elapsed)
                if row.get('boot_stage') == 'agent_ready':
                    times.setdefault('agent_ready_observed_ms', elapsed)
                    return True
                return False
            waiter.until(ready, title + ' OpenCode binding and agent_ready')
            log = host.log_since(offset, 'opencode-launch-' + title)
            stages = evidence.timing(log, tab, start_ms)
            times['daemon_stages_ms'] = stages
            times['requested_model'] = model
            host.write('opencode-timing-' + tab + '.json', times)
            if 'first-composer' not in stages:
                self.finding(tab, 'missing_composer_timing', 'No authoritative first-composer timing emitted', path)
            oracle.require('first-composer' in stages, f'{tab}: first-composer timing unavailable')
            if max(times['session_binding_observed_ms'], times['agent_ready_observed_ms']) > 15000:
                self.finding(tab, 'slow_readiness', times, 'opencode-timing-' + tab + '.json')
            host.wait(lambda: self.startup_ack(tab), title + ' own startup ACK')
            if worker:
                host.wait(lambda: host.acked(tab), title + ' task ACK')
            host.wait(lambda: any(startup in text for text in evidence.texts(self.chat(tab, 'opencode-startup-' + tab), 'assistant')), title + ' startup transcript')
        before = {tab: host.inspect(tab) for tab in self.tabs}
        oracle.bindings(before, provider='opencode')
        host.write('opencode-spawn.json', before)
        self.browser('open', 'open', 'http://127.0.0.1:7692/#token=' + os.environ['CODETERM_API_TOKEN'])
        self.browser_open = True
        self.browser('ready', 'wait', '--selector', '[data-testid=sidebar]')
        for tab, row in before.items():
            self.select(tab)
            self.visible(tab, 'RL-OPENCODE-START-' + row['title'], 'startup-' + tab)
            state = observe.status(tab) or {}
            host.write('opencode-model-status-' + tab + '.json', state)
            actual = state.get('model') or row.get('agent_session_model')
            if actual != self.expected[tab]:
                self.finding(tab, 'model_display_unverified', dict(expected_id=self.expected[tab], detected_display=actual), 'opencode-spawn.json')
        host.pass_phase(8, 'OpenCode: two Workers default/cheap plus plain default; timings, ACK and visible startup')

    def first_turn(self, label='first', phase=9):
        self.host.phase = phase
        markers = {tab: 'RL-OPENCODE-' + label.upper() + '-' + tab for tab in self.tabs}
        for tab, marker in markers.items():
            self.select(tab)
            waiter = observe.EventWait(int(os.environ.get('RL_TURN_DEADLINE', '240')))
            self.host.send(tab, f'Reply exactly {marker}-REPLY. Do not use tools.', 'opencode-' + label + '-' + tab)
            def replied(_):
                self.serial += 1
                rows = self.chat(tab, f'opencode-{label}-live-{self.serial}-{tab}')
                return any(marker + '-REPLY' in text for text in evidence.texts(rows, 'assistant'))
            waiter.until(replied, tab + ' own first reply')
            self.visible(tab, marker + '-REPLY', label + '-' + tab)
        chats = self.chats(label + '-after')
        oracle.exact_markers({tab: evidence.texts(rows, 'user') for tab, rows in chats.items()}, markers)
        self.host.pass_phase(phase, f'OpenCode {label}: each marked task exactly once in own session; reply visible live')

    def restart(self, working):
        host = self.host
        host.phase = 11 if working else 10
        label = 'opencode-working' if working else 'opencode-idle'
        host.idle()
        before = {tab: host.inspect(tab) for tab in self.tabs}
        chats = self.chats(label + '-before')
        if working:
            with concurrent.futures.ThreadPoolExecutor(max_workers=3) as pool:
                turns = [pool.submit(host.long_turn, tab, label) for tab in self.tabs]
                for turn in turns:
                    turn.result()
            states = {tab: observe.status(tab) for tab in self.tabs}
            host.write(label + '-working.json', states)
            oracle.require(all((row or {}).get('status', '').lower() == 'working' for row in states.values()), 'OpenCode tabs must all be Working at restart')
        offset = host.restart(label)
        after = {tab: host.inspect(tab) for tab in self.tabs}
        oracle.bindings(after, before, provider='opencode')
        host.write(label + '-bindings.json', after)
        if working:
            # Incomplete evidence keeps waiting; duplicate continues fail immediately.
            def resumed():
                latest = self.chats(label + '-progress')
                for tab, rows in latest.items():
                    delta = [row for row in rows if row['id'] not in set(evidence.row_ids(chats[tab]))]
                    count = sum(oracle.CONTINUE in text for text in evidence.texts(delta, 'user'))
                    oracle.require(count <= 1, f'{tab}: duplicated owed continue')
                    if count != 1 or not evidence.resumed(delta):
                        return False
                return True
            host.wait(resumed, 'OpenCode original work resumes after one owed continue')
        else:
            host.idle()
        latest = self.chats(label + '-final')
        for tab, rows in latest.items():
            evidence.retained(chats[tab], rows)
            evidence.continues(chats[tab], rows, int(working))
            self.select(tab)
            view = self.browser(label + '-' + tab, 'snapshot')
            if 'Starting' in view:
                self.finding(tab, 'stuck_starting', 'Chat still Starting after rebind', label)
            oracle.require('Starting' not in view, f'{tab}: Chat Starting after restart')
        log = host.log_since(offset, label)
        oracle.restart_logs(log, self.tabs, idle=not working)
        other = [tab for tab in host.tabs if tab not in self.tabs]
        oracle.restart_logs(log, other, idle=True)
        host.idle()
        self.first_turn(label + '-first-after-restart', host.phase)
        oracle.bindings({tab: host.inspect(tab) for tab in self.tabs}, before, provider='opencode')
        host.pass_phase(host.phase, 'OpenCode: same lazy sessions, retained rows, ' + ('one continue and resumed work each' if working else 'zero continues'))

    def run(self):
        try:
            self.spawn()
            self.first_turn()
            self.restart(False)
            self.restart(True)
        except BaseException as error:
            self.finding(None, 'hard_failure', str(error), 'failure.txt')
            raise
        finally:
            self.host.write('opencode-observation-limits.json', dict(
                flicker='Discrete snapshots cannot exclude transient flicker; inspect screenshots and event receipts.',
                findings=self.findings))
            if self.browser_open:
                self.browser('close', 'close')
