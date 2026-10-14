import {readFileSync, writeFileSync} from 'node:fs';
import {fileURLToPath} from 'node:url';
import {resolve} from 'node:path';
import {measurements} from './model.mjs';

export function worstCases(root, cwd) {
  const manualFiles = ['prod-docs/ORCHESTRATION.md', 'prod-docs/ROUTING.md',
    'prod-docs/TAB-LIFECYCLE.md', 'src-tauri/prompts/codeterm_cli.md',
    'src-tauri/prompts/manager_bootstrap.md', 'src-tauri/prompts/general_agent.md'];
  const manuals = manualFiles.map(path => readFileSync(resolve(root, path), 'utf8')).join('\n');
  const fullManual = 'x'.repeat(Math.max(Buffer.byteLength(manuals), 48 * 1024));
  const rows = [
    ['fixture-orchestrator', 'Manager', 'orchestrator', fullManual, fullManual],
    ['fixture-general', 'Manager', 'general_agent', fullManual, fullManual],
    ['fixture-long-history', null, null, '', ''],
    ['fixture-legacy-worker', 'Worker', null, '', 'x'.repeat(1024)],
    ['fixture-hidden-worker', 'Worker', null, '', 'x'.repeat(1024)],
    ['fixture-planner', 'Manager', 'planner', '', ''],
  ];
  const tabs = rows.map(([id, role, profile, prompt, identity]) => ({
    id, title: id.replace('fixture-', '').replaceAll('-', ''), split_tree: {
      type: 'Leaf', tab_id: id, cwd, shell_profile: null, agent_type: 'codex',
      claude_session_id: `thread-${id}`, agent_type_locked: true, role, role_profile: profile,
      team_id: id === 'fixture-general' ? null : 'team_fixture',
      parent_tab_id: ['Worker', 'Manager'].includes(role) && profile !== 'orchestrator' && profile !== 'general_agent'
        ? 'fixture-orchestrator' : null,
      can_spawn: role === 'Manager', depth: role === 'Worker' ? 2 : 1,
      audience: 'automation', chat_view: false,
      agent_identity: {
        session_binding_nonce: `nonce-${id}`, reported_session_binding_nonce: `nonce-${id}`,
        agent_runtime_config: {systemPrompt: prompt},
        restorable_launch: {captured: true, identity_instructions: identity},
        ...(['fixture-legacy-worker', 'fixture-hidden-worker'].includes(id)
          ? {task_binding: {plan_id: 'plan_fixture', task_id: `task_${id}`, bound_at: 1}} : {}),
      },
    },
  }));
  const histories = tabs.map(tab => ({tab_id: tab.id, page_size: 100, messages: tab.id === 'fixture-long-history'
    ? Array.from({length: 320}, (_, index) => ({id: `message-${index}`, seq: index + 1,
      type: index === 319 ? 'user' : 'assistant', content: 'x'.repeat(4096),
      ...(index === 319 ? {attachments: [{id: 'fixture-image', mimeType: 'image/png', data: 'x'.repeat(16384)}]} : {})})) : []}));
  const snapshot = {version: '1.14.7', timestamp: 1, active_workspace_id: 'workspace_fixture',
    teams: [{id: 'team_fixture', workspace_id: 'workspace_fixture', lifecycle: 'active', created_at: 1}],
    workspaces: [{id: 'workspace_fixture', name: 'Fixture', cwd, active_tab_id: tabs[0].id, tabs}]};
  return {format: 'upgrade-restore-v1', sanitized: true, snapshot, histories,
    measurements: measurements(snapshot, histories),
    live_tab_metadata: [{tab_id: 'fixture-hidden-worker', hidden: true, show_in_tabbar: false}],
    plans: [{id: 'plan_fixture', title: 'Fixture', status: 'active', mode: 'execution',
      owner: {tab_id: 'fixture-orchestrator', session_id: 'thread-fixture-orchestrator', title: 'Fixture'},
      team_id: 'team_fixture', project_path: cwd, canonical_project_path: cwd, created_at: 1, updated_at: 1,
      tasks: ['fixture-legacy-worker', 'fixture-hidden-worker'].map(id => ({
        id: `task_${id}`, name: 'Fixture', type: 'implementation', status: 'in_progress',
        assigned_tab_id: id, execution_intent: 'non_repository',
        execution_attempts: [{id: `attempt_${id}`, executor_tab_id: id, provider: 'codex',
          session_id: `thread-${id}`, session_binding_nonce: `nonce-${id}`, role: 'Worker',
          parent_tab_id: 'fixture-orchestrator', orchestrator_tab_id: 'fixture-orchestrator',
          assigned_at: 1, acknowledged_at: 2, closed_at: null, retired_at: null}],
      }))}],
    fixture_contract: {manual_files: manualFiles, required_repairs: ['desk leaf bound', 'SizeLimit', 'binding incomplete']}};
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const [out, root, cwd] = process.argv.slice(2);
  if (!out || !root || !cwd) throw new Error('Usage: node fixtures.mjs OUT ROOT ISOLATED_CWD');
  writeFileSync(out, JSON.stringify(worstCases(root, cwd), null, 2));
}
