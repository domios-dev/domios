import json
import os
from pathlib import Path
import signal
import subprocess
import time

from receipts import digest, redact, save

WARNING = 'Under-development features enabled: '


def decode_codex(raw):
    events = [json.loads(line) for line in raw.splitlines() if line.strip()]
    sessions, usage, messages = [], [], []
    first_message = False
    for event in events:
        kind = event.get('type')
        if kind == 'thread.started':
            sessions.append(event['thread_id'])
        if kind in ('error', 'turn.failed'):
            raise ValueError(f'Codex failed event: {kind}')
        if kind == 'turn.completed':
            usage.append(event['usage'])
        if kind == 'item.completed':
            item = event.get('item', {})
            if item.get('type') == 'agent_message':
                first_message = True
                messages.append(item['text'])
            elif item.get('type') == 'error':
                if first_message or not item.get('message', '').startswith(WARNING):
                    raise ValueError('non-startup-warning Codex error item')
            elif item.get('type') == 'command_execution' and item.get('exit_code', 0) != 0:
                raise ValueError('Codex command failed; no retry cohort')
    if len(sessions) != 1 or not messages or not usage:
        raise ValueError('missing fresh Codex session, completion or usage')
    totals = {key: sum(u.get(key, 0) for u in usage) for key in
              ('input_tokens', 'cached_input_tokens', 'cache_write_input_tokens', 'output_tokens', 'reasoning_output_tokens')}
    normalized = {'input_tokens': totals['input_tokens'] - totals['cached_input_tokens'] - totals['cache_write_input_tokens'],
                  'cache_read_input_tokens': totals['cached_input_tokens'],
                  'cache_creation_input_tokens': totals['cache_write_input_tokens'],
                  'output_tokens': totals['output_tokens'], 'reasoning_output_tokens': totals['reasoning_output_tokens']}
    if normalized['input_tokens'] < 0:
        raise ValueError('invalid disjoint Codex token accounting')
    return {'session_id': sessions[0], 'usage': normalized, 'cli_usage': usage, 'result': messages[-1]}


def decode_claude(raw, cwd):
    events = [json.loads(line) for line in raw.splitlines() if line.strip()]
    initial = [e for e in events if e.get('type') == 'system' and e.get('subtype') == 'init']
    results = [e for e in events if e.get('type') == 'result']
    if len(initial) != 1 or len(results) != 1:
        raise ValueError('missing fresh Claude init or final result')
    init, result = initial[0], results[0]
    if Path(init['cwd']).resolve() != Path(cwd).resolve() or set(init.get('tools', [])) != {'Bash'}:
        raise ValueError('Claude cwd/tool scope differs from the registered Bash-only case')
    if init.get('mcp_servers') or init.get('skills') or init.get('plugins'):
        raise ValueError('extra Claude MCP/skill/plugin context present')
    if result.get('is_error') or result.get('permission_denials') or result.get('subtype') != 'success':
        raise ValueError('Claude refused or failed its bounded session')
    if not result.get('session_id') or not result.get('usage'):
        raise ValueError('missing Claude session/usage evidence')
    return {'session_id': result['session_id'], 'usage': result['usage'],
            'modelUsage': result.get('modelUsage'), 'result': result.get('result')}


def provider_argv(provider, binary, model):
    if provider == 'claude':
        return [binary, '-p', '--model', model, '--effort', 'high', '--output-format', 'stream-json', '--verbose',
                '--safe-mode', '--setting-sources', '', '--strict-mcp-config', '--mcp-config', '{"mcpServers":{}}',
                '--disable-slash-commands', '--no-session-persistence', '--no-chrome',
                '--tools', 'Bash', '--allowedTools', 'Bash', '--permission-mode', 'dontAsk', '--permission-prompts', 'none']
    argv = [binary, '--no-daemon', '-a', 'never', 'exec', '--json', '-m', 'gpt-6.1-sol',
            '--sandbox', 'workspace-write', '--ignore-user-config', '--ignore-rules', '--skip-git-repo-check', '--ephemeral']
    for setting in ['model_reasoning_effort="high"', 'project_doc_max_bytes=0', 'web_search="disabled"',
                    'features.memories=false', 'memories.generate_memories=false', 'memories.use_memories=false',
                    'features.hooks=false', 'features.apps=false', 'features.plugins=false', 'features.remote_plugin=false',
                    'features.multi_agent=false', 'features.multi_agent_v2=false', 'agents.enabled=false',
                    'features.skill_search=false', 'features.skip_host_skill_discovery=true',
                    'features.browser_use=false', 'features.computer_use=false', 'features.image_generation=false',
                    'features.goals=false', 'mcp_servers={}', 'shell_environment_policy.inherit="all"']:
        argv += ['-c', setting]
    return argv + ['-']


def subject_environment(proxy, binaries):
    paths = [str(proxy), *(str(Path(binary).parent) for binary in binaries), '/opt/homebrew/bin', '/usr/bin', '/bin']
    return {'HOME': str(Path.home()), 'PATH': ':'.join(dict.fromkeys(paths)), 'LANG': 'en_US.UTF-8',
            'CODETERM_DEV_INSTANCE': 'blueprint-v2-proof', 'CODETERM_DEV_API_PORT': '17693'}


def verify_help(provider, binary, env, directory):
    surfaces = [([], ['--safe-mode', '--tools', '--allowedTools', '--setting-sources', '--strict-mcp-config',
                       '--permission-prompts', '--permission-mode', '--no-session-persistence', '--no-chrome',
                       '--disable-slash-commands', '--effort', '--output-format', '--verbose', '--mcp-config'])]
    if provider == 'codex':
        surfaces = [([], ['--no-daemon', '--ask-for-approval']),
                    (['exec'], ['--ignore-user-config', '--ignore-rules', '--sandbox', '--ephemeral', '--json',
                                '--skip-git-repo-check', '--model', '--config'])]
    for subcommand, required in surfaces:
        command = [binary, *subcommand, '--help']
        result = subprocess.run(command, env=env, text=True, capture_output=True, timeout=30)
        label = '-'.join([provider, *subcommand, 'help'])
        save(directory / f'{label}.json', {'argv': command, 'exit': result.returncode,
                                          'stdout': result.stdout, 'stderr': result.stderr})
        if result.returncode or any(flag not in result.stdout for flag in required):
            raise ValueError(f'{provider} lacks required clean headless launch capability')


def launch(provider, binary, model, prompt, cwd, proxy, binaries, directory):
    if list(cwd.iterdir()):
        raise ValueError('subject cwd is not empty')
    env = subject_environment(proxy, binaries)
    if sorted(k for k in env if k.startswith('CODETERM')) != ['CODETERM_DEV_API_PORT', 'CODETERM_DEV_INSTANCE']:
        raise ValueError('unexpected subject connection context')
    argv = provider_argv(provider, binary, model)
    record = {'provider': provider, 'model': model, 'effort': 'high', 'class': 'Worker-class author',
              'argv': argv, 'cwd': str(cwd), 'cwd_initial_entries': [], 'env_keys': sorted(env),
              'prompt_sha256': digest(prompt), 'started_at': time.time(), 'deadline_seconds': 900, 'fresh': True}
    directory.mkdir(parents=True, exist_ok=True)
    with (directory / 'started.json').open('x') as stream:
        json.dump(record, stream, indent=2)
    (directory / 'prompt.md').write_text(prompt)
    process = subprocess.Popen(['/usr/bin/env', '-i', *(f'{key}={value}' for key, value in env.items()), *argv],
                               cwd=cwd, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                               text=True, start_new_session=True)
    record['pid'] = process.pid
    try:
        stdout, stderr = process.communicate(prompt, timeout=900)
        record['exit'] = process.returncode
    except subprocess.TimeoutExpired:
        os.killpg(process.pid, signal.SIGTERM)
        try:
            stdout, stderr = process.communicate(timeout=5)
        except subprocess.TimeoutExpired:
            os.killpg(process.pid, signal.SIGKILL)
            stdout, stderr = process.communicate(timeout=5)
        record['exit'] = 'deadline'
    record['seconds'] = time.time() - record['started_at']
    (directory / 'stdout.jsonl').write_text(redact(stdout))
    (directory / 'stderr.txt').write_text(redact(stderr))
    if record['exit'] == 0:
        try:
            record['data'] = decode_codex(stdout) if provider == 'codex' else decode_claude(stdout, cwd)
        except (ValueError, KeyError, TypeError) as error:
            record['decode_error'] = str(error)
    save(directory / 'session.json', record)
    if record['exit'] != 0 or 'decode_error' in record:
        raise ValueError(f'{provider} session failed/refused: {record.get("decode_error", record["exit"])}')
    return record
