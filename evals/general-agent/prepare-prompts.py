import argparse
import json
from pathlib import Path
import subprocess


def at_revision(root: Path, revision: str, path: str) -> str:
    return subprocess.run(
        ['git', 'show', f'{revision}:{path}'],
        cwd=root, check=True, capture_output=True, text=True, encoding='utf-8',
    ).stdout


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument('--baseline-ref', required=True)
    parser.add_argument('--output-dir', type=Path, required=True)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[2]
    args.output_dir.mkdir(parents=True, exist_ok=True)
    families = {
        'general-agent': [
            'general_agent.md', 'codeterm_rule.md', 'bridge_mem.md',
        ],
        'bootstrap': [
            'general_agent.md', 'codeterm_rule.md', 'bridge_mem.md',
            'bootstrap_instructions.md',
        ],
        'spawned-agent': [
            'agent_codeterm_primer.md', 'agent_pad_rule.md',
            'rich_rendering_guidance.md',
        ],
    }
    changed = {
        'bootstrap_instructions.md', 'codeterm_rule.md',
        'agent_codeterm_primer.md', 'rich_rendering_guidance.md',
    }
    for family, names in families.items():
        old, new = [], []
        for name in names:
            path = f'src-tauri/prompts/{name}'
            baseline = at_revision(root, args.baseline_ref, path).strip()
            old.append(baseline)
            new.append((root / path).read_text(encoding='utf-8').strip() if name in changed else baseline)
        for arm, fragments in [('old', old), ('new', new)]:
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
