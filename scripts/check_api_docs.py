"""Compare production Axum registration sites with embedded API help."""
from pathlib import Path
import json
import re
import sys

METHODS = r"GET|POST|PUT|PATCH|DELETE|HEAD|OPTIONS|TRACE|CONNECT"
# Mask comments, preserving quoted paths and character offsets.
RUST_TOKEN = re.compile(r'"(?:\\.|[^"\\])*"|//[^\n]*|/\*.*?\*/', re.S)


def production_text(text):
    text = re.sub(r'r(#+)".*?"\1', lambda m: ' ' * len(m[0]), text, flags=re.S)
    text = re.sub(r"'(?:\\.|[^'\\])'", lambda m: ' ' * len(m[0]), text)
    text = RUST_TOKEN.sub(
        lambda m: m[0] if m[0].startswith('"') else ' ' * len(m[0]), text
    )
    # Remove only inline test modules, not the production code after a
    # test-only helper or an external `mod tests;` declaration.
    pattern = re.compile(r'#\[cfg\(test\)\]\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+\w+\s*\{')
    while match := pattern.search(text):
        end = balanced_end(text, match.end(), '{', '}')
        text = text[:match.start()] + ' ' * (end - match.start()) + text[end:]
    return text


def balanced_end(text, pos, opening='(', closing=')'):
    depth = 1
    while pos < len(text):
        if text[pos] == '"':
            match = re.match(r'"(?:\\.|[^"\\])*"', text[pos:])
            if not match:
                raise ValueError('Unterminated Rust string')
            pos += match.end()
            continue
        if text[pos] == opening:
            depth += 1
        elif text[pos] == closing:
            depth -= 1
            if depth == 0:
                return pos + 1
        pos += 1
    raise ValueError('Unterminated Rust registration or test module')


def route_calls(text):
    for match in re.finditer(r'\.route\s*\(', text):
        end = balanced_end(text, match.end())
        yield text[match.end():end - 1]


def normalize(path):
    path = path.split('?', 1)[0].strip()
    path = re.sub(r'\{(\*?)[^}/]+\}', lambda m: '{*}' if m[1] else '{}', path)
    return re.sub(r'/:[^/]+', '/{}', path)


def registered_routes(root):
    source_dir = root / 'src-tauri/crates/codeterm-api/src'
    registered = set()
    sources = []
    # Include split routers automatically. Test fixture routers are not API
    # registrations; inline test modules are removed without truncating files.
    for source in sorted(source_dir.rglob('*.rs')):
        parts = source.relative_to(source_dir).parts
        if any(p in ('tests', 'fixtures') or p.endswith('_tests') or p == 'fixture.rs' or re.search(r'(^|_)tests?\.rs$', p) for p in parts):
            continue
        raw = source.read_text(encoding='utf-8')
        if '.route' not in raw:
            continue
        text = production_text(raw)
        calls = list(route_calls(text))
        if calls:
            sources.append(source.relative_to(source_dir).as_posix())
        for call in calls:
            match = re.fullmatch(r'\s*("[^"]+"|[\w:]+)\s*,\s*(.*?)\s*,?\s*', call, re.S)
            if not match:
                raise ValueError(f'Unrecognized route registration in {source}: {call}')
            path, expr = match.groups()
            if path.startswith('"'):
                path = path[1:-1]
            else:
                parts = path.split('::')
                owner = source_dir.joinpath(*parts[:-1]).with_suffix('.rs')
                if parts[0].startswith('codeterm_'):
                    owner = source_dir.parent.parent / parts[0].replace('_', '-') / 'src'
                    owner = owner.joinpath(*parts[1:-1]).with_suffix('.rs')
                if not owner.exists():
                    owner = owner.with_suffix('') / 'mod.rs'
                constant = re.search(r'const\s+' + re.escape(parts[-1]) + r'\s*:\s*&str\s*=\s*"([^"]+)"', owner.read_text(encoding='utf-8'))
                if not constant:
                    raise ValueError(f'Cannot resolve route constant {path}')
                path = constant[1]
            # Wildcard any handlers are startup fallbacks, not API endpoints.
            if path.endswith('/{*path}') and re.search(r'(?:^|[.\s(])(?:axum::routing::)?any\s*\(', expr):
                continue
            methods = re.findall(r'(?:^|[.\s(])(?:axum::routing::)?(' + METHODS.lower() + r')\s*\(', expr)
            if not methods:
                raise ValueError(f'No recognized HTTP method for {path} in {source}')
            registered.update((method.upper(), normalize(path)) for method in methods)
    if not registered:
        raise ValueError('No production routes found')
    return registered, sources


def documented_routes(root):
    documented = set()
    for source in sorted((root / 'prod-docs').glob('*.md')):
        for line in source.read_text(encoding='utf-8').splitlines():
            match = re.match(r'\|\s*(' + METHODS + r')\s*\|\s*`([^`]+)`', line)
            if match:
                documented.add((match[1], normalize(match[2])))
    if not documented:
        raise ValueError('No documented routes found')
    return documented


def drift(root):
    registered, sources = registered_routes(root)
    documented = documented_routes(root)
    entries = json.loads((root / 'scripts/api-docs-internal.json').read_text(encoding='utf-8'))
    internal = set()
    for entry in entries:
        if set(entry) != {'method', 'path', 'reason'} or not entry['reason'].strip():
            raise ValueError(f'Internal route needs method, path and reason: {entry}')
        route = (entry['method'], normalize(entry['path']))
        if route in internal or route not in registered:
            raise ValueError(f'Duplicate or unregistered internal route: {route}')
        internal.add(route)
    public = registered - internal
    return public - documented, documented - registered, public, documented - internal, sources


def main(root):
    missing, stale, public, documented, sources = drift(root)
    print(f'production router sources: {len(sources)}')
    print(f'registered public routes: {len(public)}')
    print(f'documented routes: {len(documented)}')
    for title, routes in [('Implemented but undocumented', missing), ('Documented but unregistered', stale)]:
        if routes:
            print(f'\n{title}:')
            for method, path in sorted(routes):
                print(f'{method} {path}')
    if not missing and not stale:
        print('\nAPI docs are in sync with registered public routes.')
    return int(bool(missing or stale))


if __name__ == '__main__':
    try:
        sys.exit(main(Path(sys.argv[1]) if len(sys.argv) > 1 else Path(__file__).resolve().parent.parent))
    except (OSError, ValueError) as error:
        print(f'API docs check failed: {error}', file=sys.stderr)
        sys.exit(1)
