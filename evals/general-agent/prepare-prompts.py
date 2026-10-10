import argparse
import json
from pathlib import Path
import subprocess


def at_revision(root: Path, revision: str, path: str) -> str:
    return subprocess.run(
        ['git', 'show', f'{revision}:{path}'],
        cwd=root, check=True, capture_output=True, text=True, encoding='utf-8',
    ).stdout


def general_agent(source) -> str:
    template = source('general_agent.md')
    cli = source('codeterm_cli.md')
    before, marker, after = cli.partition('## Memory (mem)')
    if marker and '## Platform notes' in after:
        cli = before + '## Platform notes' + after.partition('## Platform notes')[2]
    template = template.replace('{codeterm_cli}', cli + '\n' + source('agent_pad_rule.md'))
    if '{interactive_child_pty_operation}' in template:
        template = template.replace('{interactive_child_pty_operation}', source('interactive_child_pty.md'))
    return '\n\n---\n\n'.join([
        source('agent_codeterm_primer.md'), template,
        source('codeterm_rule.md'), source('bridge_mem.md'),
    ])


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument('--baseline-ref', required=True)
    parser.add_argument('--output-dir', type=Path, required=True)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[2]
    args.output_dir.mkdir(parents=True, exist_ok=True)
    families = {
        'general-agent': [],
        'bootstrap': ['bootstrap_instructions.md'],
        'spawned-agent': [
            'agent_codeterm_primer.md', 'agent_pad_rule.md',
            'rich_rendering_guidance.md',
        ],
    }
    for family, names in families.items():
        for arm in ['old', 'new']:
            def source(name: str) -> str:
                path = f'src-tauri/prompts/{name}'
                if arm == 'old':
                    return at_revision(root, args.baseline_ref, path).strip()
                return (root / path).read_text(encoding='utf-8').strip()

            fragments = [] if family == 'spawned-agent' else [general_agent(source)]
            fragments.extend(source(name) for name in names)
            if family == 'spawned-agent':
                path = 'src-tauri/prompts/utility_tab_guidance.md'
                present = (root / path).exists() if arm == 'new' else subprocess.run(
                    ['git', 'cat-file', '-e', f'{args.baseline_ref}:{path}'],
                    cwd=root, capture_output=True,
                ).returncode == 0
                if present:
                    fragments.insert(1, source('utility_tab_guidance.md'))
            destination = args.output_dir / f'{family}-{arm}.md'
            destination.write_text('\n\n'.join(fragments) + '\n', encoding='utf-8')
            print(destination)
    source = json.loads((root / 'evals/general-agent/evals.json').read_text(encoding='utf-8'))
    for family in ['general-agent', 'bootstrap']:
        fixture = {**source, 'evals': [
            item for item in source['evals']
            if item.get('prompt_family', 'general-agent') == family
        ]}
        destination = args.output_dir / f'{family}-evals.json'
        destination.write_text(json.dumps(fixture, indent=2) + '\n', encoding='utf-8')
        print(destination)


if __name__ == '__main__':
    main()
