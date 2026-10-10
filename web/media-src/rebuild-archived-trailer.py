import argparse
import hashlib
import json
import shutil
import subprocess
import sys
from pathlib import Path


def digest(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def verify(project, git_ref=None):
    inventory = json.loads((project / 'inventory.json').read_text(encoding='utf-8'))
    for item in inventory['files']:
        path = (project / item['path']).resolve()
        if not path.is_relative_to(project.resolve()):
            raise ValueError('Archive path escapes the project')
        if path.stat().st_size != item['bytes'] or digest(path) != item['sha256']:
            raise ValueError(f"Archived input changed: {item['path']}")
        if git_ref:
            repo = Path(__file__).resolve().parents[2]
            spec = f'{git_ref}:{path.relative_to(repo).as_posix()}'
            blob = subprocess.run(['git', 'cat-file', 'blob', spec], cwd=repo,
                                  check=True, stdout=subprocess.PIPE, stdin=subprocess.DEVNULL).stdout
            if len(blob) != item['bytes'] or hashlib.sha256(blob).hexdigest() != item['sha256']:
                raise ValueError(f"Git archive bytes differ: {item['path']}")
    return inventory


def relocate(value, locations):
    if isinstance(value, str):
        return locations.get(value.replace('\\', '/').lower(), value)
    if isinstance(value, list):
        return [relocate(item, locations) for item in value]
    if isinstance(value, dict):
        return {key: relocate(item, locations) for key, item in value.items()}
    return value


def main():
    parser = argparse.ArgumentParser(description='Rebuild the selected trailer from Git-owned inputs')
    parser.add_argument('--project', type=Path, default=Path(__file__).resolve().parent / 'projects/domios-memory-2026-10-03')
    parser.add_argument('--verify-only', action='store_true')
    parser.add_argument('--git-ref')
    parser.add_argument('--output', type=Path)
    parser.add_argument('--render-picture', action='store_true')
    parser.add_argument('--font', type=Path)
    args = parser.parse_args()
    project = args.project.resolve()
    inventory = verify(project, args.git_ref)
    print(f"Verified {len(inventory['files'])} archived files", flush=True)
    if args.verify_only:
        return
    if args.output is None:
        parser.error('--output is required unless --verify-only is supplied')
    output = args.output.resolve()
    if output.is_relative_to(project) or project.is_relative_to(output):
        parser.error('Output must be separate from the immutable archive')
    output.mkdir(parents=True, exist_ok=False)
    tools = Path(__file__).resolve().parent
    prepared = project / 'prepared'
    picture = project / 'picture'
    if args.render_picture:
        if args.font is None or not args.font.is_file():
            parser.error('--render-picture requires an installed --font file')
        prepared_copy = output / 'prepared'
        shutil.copytree(prepared, prepared_copy)
        prepared = prepared_copy
        picture = output / 'picture'
        subprocess.run([sys.executable, str(tools / 'render-memory-extension.py'),
                        '--baseline', str(project / 'baseline-picture'), '--prepared', str(prepared),
                        '--source-dir', str(project / 'recordings'), '--brand-frames', str(project / 'brand'),
                        '--font', str(args.font), '--output', str(picture)], check=True, stdin=subprocess.DEVNULL)
    locations = {item['originalLocation'].replace('\\', '/').lower(): str(project / item['path'])
                 for item in inventory['files']}
    credit = json.loads((prepared / 'provenance.json').read_text(encoding='utf-8'))
    credit_path = output / 'music-provenance.json'
    credit_path.write_text(json.dumps(relocate(credit, locations), indent=2) + '\n', encoding='utf-8')
    subprocess.run([sys.executable, str(tools / 'mux-narration.py'), '--video-dir', str(picture),
                    '--audio', str(prepared / 'narration-extended.wav'),
                    '--alignment', str(prepared / 'alignment.json'), '--timing', str(prepared / 'timing.json'),
                    '--music', str(prepared / 'prepared-music.wav'), '--music-metadata', str(credit_path),
                    '--music-prepared', '--opus-frame-duration', '10',
                    '--output-dir', str(output / 'narrated'), '--proof-dir', str(output / 'sound-proof')],
                   check=True, stdin=subprocess.DEVNULL)
    print(f"Rebuilt trailer: {output / 'narrated/trailer.mp4'}")


if __name__ == '__main__':
    main()
