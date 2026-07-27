"""Auto-split persona seeds (was inline in loaded.py).

Use _add() to append to this module's ITEMS — each persona keeps its own list,
which the loaded package concatenates in __init__.py.
"""

ITEMS: list[dict] = []


def _add(**kw):
    ITEMS.append(kw)


# ─────────────────────────────────────────────────────────────
# COBALT (internal React enterprise platform) — ~25 items
# ─────────────────────────────────────────────────────────────

_add(kind="fact", title="Cobalt stack",
    body="Cobalt frontend is React 18 + TypeScript + Vite + TanStack Query + Zustand. Tailwind for styling. Playwright for E2E tests.",
    area="cobalt", importance=4, tags=["react", "architecture"])

_add(kind="fact", title="Cobalt auth flow",
    body="Cobalt uses SAML SSO against Okta. Session cookie is httpOnly, SameSite=Lax, 12h expiry. No refresh token — full re-auth on expiry.",
    area="cobalt", importance=4, tags=["auth"])

_add(kind="learning", title="TanStack Query stale while revalidate",
    body="TanStack Query refetches on window focus by default. Caused duplicate expensive API calls. Set refetchOnWindowFocus: false globally, opt-in per query.",
    area="cobalt", importance=3, tags=["react-query"])

_add(kind="learning", title="Zustand persist middleware SSR",
    body="Zustand persist middleware reads localStorage synchronously in the initializer. Breaks SSR hydration. Wrap with typeof window check or use onRehydrateStorage.",
    area="cobalt", importance=3, tags=["zustand", "ssr"])

_add(kind="note", title="Riley proposed dashboard redesign",
    body="Riley wants to split the main dashboard into tabls that can be rearranged. Uses react-grid-layout. Mock in Figma approved by team.",
    area="cobalt", importance=2, tags=["design"])

_add(kind="skill", title="Cobalt local dev",
    body="pnpm install && pnpm dev. Runs on http://localhost:5173. Mock API proxied to staging via vite.config.ts dev proxy.",
    area="cobalt", importance=3, tags=["dev"])

_add(kind="skill", title="Cobalt Playwright tests",
    body="pnpm test:e2e runs Playwright against local dev server. CI runs against a preview deployment. Flaky tests auto-retry up to 2x.",
    area="cobalt", importance=2, tags=["testing"])

_add(kind="rule", title="No untyped any in Cobalt",
    body="Cobalt eslint config forbids any. Use unknown + narrowing or proper generics. CI blocks PRs with any.",
    area="cobalt", importance=3, tags=["typescript", "rule"])

_add(kind="preference", title="Cobalt commit style",
    body="Conventional commits: feat/fix/chore/docs/test/refactor. Scope is optional but encouraged for multi-module changes.",
    area="cobalt", importance=2, tags=["git"])

_add(kind="fact", title="Cobalt bundle size",
    body="Cobalt production bundle is 420KB gzipped. Main chunk 180KB, vendor 240KB. Tracked via rollup-plugin-visualizer.",
    area="cobalt", importance=3, tags=["perf"])

_add(kind="learning", title="Vite glob import performance",
    body="Using import.meta.glob('./routes/*.tsx', { eager: true }) loaded all 80 route modules on boot and tripled startup time. Switched to lazy glob + React.lazy.",
    area="cobalt", importance=3, tags=["vite", "perf"])

_add(kind="learning", title="React 18 concurrent rendering pitfall",
    body="useEffect runs twice in StrictMode dev only. Side effects (analytics events, API calls with no cache key) must be idempotent or guarded with a ref.",
    area="cobalt", importance=3, tags=["react"])

_add(kind="note", title="Playwright flake investigation",
    body="E2E login test fails ~5% runs. Root cause: Okta login iframe sometimes takes >3s to load. Bumped waitForSelector timeout to 15s.",
    area="cobalt", importance=2, tags=["testing"])

_add(kind="fact", title="Cobalt deployment",
    body="Cobalt deploys to Cloudfront via S3. Main branch auto-deploys to staging, release branch to prod. CDN cache invalidated on deploy.",
    area="cobalt", importance=3, tags=["deploy"])

_add(kind="skill", title="Cobalt CSS migration",
    body="When moving a component from CSS modules to Tailwind, use the codemod in scripts/to-tailwind.mjs. Handles ~80% of class mappings, rest manual.",
    area="cobalt", importance=2, tags=["css"])

_add(kind="conversation", title="Riley asked about Server Components",
    body="Riley asked if Cobalt should adopt React Server Components. Team decided: not yet, our API boundary is clean and RSC adds complexity for no clear win.",
    area="cobalt", importance=2, tags=["planning"])

_add(kind="fact", title="Cobalt test coverage",
    body="Unit test coverage is 68%. E2E covers the critical user flows (login, dashboard, 3 main CRUD pages). Target for Q2 is 80% unit.",
    area="cobalt", importance=2, tags=["testing"])

# ─────────────────────────────────────────────────────────────
# EXTRA cobalt — perf, tooling
# ─────────────────────────────────────────────────────────────

_add(kind="skill", title="Cobalt bundle analyze",
    body="pnpm build:analyze produces a treemap of bundle contents. Serves at http://localhost:8888. Useful for finding unexpectedly large dependencies.",
    area="cobalt", importance=3, tags=["perf", "tooling"])

_add(kind="learning", title="Tailwind JIT purge",
    body="Tailwind JIT purges unused classes only if file paths in tailwind.config.js cover them. Missed src/plugins/* and half the plugin UI was unstyled in prod.",
    area="cobalt", importance=3, tags=["tailwind", "gotcha"])

_add(kind="note", title="Bundle size investigation",
    body="Found that lodash was imported in 12 places via barrel file, pulling the whole lib. Switched to per-function imports (lodash/pick, lodash/debounce). Saved 60KB.",
    area="cobalt", importance=3, tags=["perf"])

_add(kind="fact", title="Cobalt error boundary",
    body="Top-level error boundary catches React rendering errors, shows a friendly UI with a retry button, logs to Sentry with component stack.",
    area="cobalt", importance=3, tags=["react"])

_add(kind="learning", title="Zustand shallow compare",
    body="useStore((s) => ({a: s.a, b: s.b})) creates a new object every render and causes infinite loops. Use shallow comparator: useStore(selector, shallow).",
    area="cobalt", importance=4, tags=["zustand", "gotcha"])

_add(kind="rule", title="Keep Cobalt PRs under 500 lines",
    body="Large PRs get bounced back with a request to split. Review quality drops above 500 lines of diff. Exception: pure refactors with automated codemods.",
    area="cobalt", importance=3, tags=["review", "rule"])
