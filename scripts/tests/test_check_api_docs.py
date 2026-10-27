"""Subprocess contracts for the API documentation drift gate."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

CHECKER = Path(__file__).resolve().parents[1] / 'check_api_docs.py'


class ApiDocsGateTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='api-docs-contract-')
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.source_dir = self.root / 'src-tauri/crates/codeterm-api/src'
        self.write('scripts/api-docs-internal.json', '[]')
        self.source('routes.rs', 'Router::new().route("/api/health", get(health))')
        self.write('prod-docs/API.md', '| GET | `/api/health` | Health |\n')

    def write(self, path, text):
        target = self.root / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(text, encoding='utf-8')

    def source(self, path, text):
        self.write(self.source_dir.relative_to(self.root) / path, text)

    def check(self, expected, message):
        result = subprocess.run([sys.executable, str(CHECKER), str(self.root)],
                                capture_output=True, text=True, timeout=10)
        output = result.stdout + result.stderr
        self.assertEqual(result.returncode, expected, output)
        self.assertIn(message, output)
        return output

    def test_clean_routes_accept_parameter_names_and_query_examples(self):
        self.source('tabs/router.rs', 'Router::new().route("/api/tabs/{id}", get(read).patch(edit))')
        self.write('prod-docs/API-TABS.md', '| GET | `/api/tabs/{tab}?limit=1` | Read |\n| PATCH | `/api/tabs/{tab}` | Edit |')
        self.check(0, 'registered public routes: 3')

    def test_new_split_router_fails_without_documentation(self):
        self.source('new_area/mod.rs', 'Router::new().route("/api/new", post(create))')
        self.check(1, 'POST /api/new')

    def test_stale_documentation_fails(self):
        self.write('prod-docs/API-OLD.md', '| DELETE | `/api/removed` | Old |')
        self.check(1, 'Documented but unregistered:\nDELETE /api/removed')

    def test_test_helper_and_external_module_do_not_truncate_routes(self):
        self.source('routes.rs', '#[cfg(test)]\nfn helper() {}\n#[cfg(test)]\nmod tests;\nRouter::new().route("/api/health", get(health)).route("/api/later", post(later))')
        self.check(1, 'POST /api/later')

    def test_inline_tests_and_test_files_do_not_register_routes(self):
        self.source('area.rs', '#[cfg(test)]\nmod tests { fn example() { Router::new().route("/fake", get(fake)); } }\nRouter::new().route("/api/after", get(after))')
        for path in ('tests.rs', 'area/tests.rs', 'area_tests.rs', 'area_tests/nested.rs'):
            self.source(path, 'Router::new().route("/fake", get(fake))')
        self.write('prod-docs/API-AFTER.md', '| GET | `/api/after` | After |')
        self.check(0, 'registered public routes: 2')

    def test_comments_and_raw_test_strings_are_not_routes(self):
        self.source('area.rs', '''// .route("/comment", get(fake))
/* .route("/block", get(fake)) */
#[cfg(test)] mod tests { const DATA: &str = r#"quoted \" } ("#; }
Router::new().route("/api/after", post(after))''')
        self.write('prod-docs/API-AFTER.md', '| POST | `/api/after` | After |')
        self.check(0, 'registered public routes: 2')

    def test_constant_paths_resolve_and_drift_is_rejected(self):
        self.source('routes.rs', 'Router::new().route(ci::PATH, post(receive))')
        self.source('ci.rs', 'pub const PATH: &str = "/api/ci/webhook";')
        self.write('prod-docs/API.md', '| POST | `/api/ci/webhook` | CI |')
        self.check(0, 'API docs are in sync')
        self.write('prod-docs/API.md', '| GET | `/api/health` | Health |')
        self.check(1, 'POST /api/ci/webhook')

    def test_cross_crate_constant_path_resolves(self):
        self.source('routes.rs', 'Router::new().route(codeterm_service_core::service::web::PATH, post(receive))')
        self.write('src-tauri/crates/codeterm-service-core/src/service/web.rs',
                   'pub const PATH: &str = "/api/web/notice";')
        self.write('prod-docs/API.md', '| POST | `/api/web/notice` | Notice |')
        self.check(0, 'API docs are in sync')

    def test_other_embedded_help_counts(self):
        self.source('routes.rs', 'Router::new().route("/api/account/status", get(status))')
        self.write('prod-docs/API.md', '# API\n')
        self.write('prod-docs/ACCOUNT.md', '| GET | `/api/account/status` | Status |')
        self.check(0, 'documented routes: 1')

    def test_internal_allowlist_is_exact_and_method_specific(self):
        self.source('debug.rs', 'Router::new().route("/api/debug/private", get(read).post(write)).route("/api/debug/new", get(new))')
        self.write('scripts/api-docs-internal.json', json.dumps([
            dict(method='GET', path='/api/debug/private', reason='Test-only inspection')
        ]))
        output = self.check(1, 'POST /api/debug/private')
        self.assertIn('GET /api/debug/new', output)
        self.assertNotIn('\nGET /api/debug/private', output)

    def test_stale_internal_allowlist_entry_fails(self):
        self.write('scripts/api-docs-internal.json', json.dumps([
            dict(method='GET', path='/api/removed', reason='Internal')
        ]))
        self.check(1, 'unregistered internal route')

    def test_allowlist_requires_a_reason(self):
        self.write('scripts/api-docs-internal.json', '[{"method":"GET","path":"/api/health","reason":""}]')
        self.check(1, 'needs method, path and reason')

    def test_changed_http_method_fails(self):
        self.source('routes.rs', 'Router::new().route("/api/health", post(health))')
        output = self.check(1, 'POST /api/health')
        self.assertIn('GET /api/health', output)

    def test_wildcard_is_distinct_from_single_segment(self):
        self.source('routes.rs', 'Router::new().route("/api/files/{*path}", get(read))')
        self.write('prod-docs/API.md', '| GET | `/api/files/{name}` | Read |')
        self.check(1, 'GET /api/files/{*}')

    def test_unknown_registration_is_an_error(self):
        self.source('area.rs', 'Router::new().route(dynamic_path(), get(read))')
        self.check(1, 'Unrecognized route registration')

    def test_no_routes_or_no_docs_cannot_pass(self):
        self.source('routes.rs', '// no routes')
        self.check(1, 'No production routes found')
        self.source('routes.rs', 'Router::new().route("/api/health", get(health))')
        self.write('prod-docs/API.md', '# API\n')
        self.check(1, 'No documented routes found')


if __name__ == '__main__':
    unittest.main()
