"""Auto-split persona seeds (was inline in loaded.py).

Use _add() to append to this module's ITEMS — each persona keeps its own list,
which the loaded package concatenates in __init__.py.
"""

ITEMS: list[dict] = []


def _add(**kw):
    ITEMS.append(kw)


# ─────────────────────────────────────────────────────────────
# AURORA (wellness audio iOS app) — ~40 items
# ─────────────────────────────────────────────────────────────

_add(kind="fact", title="Aurora audio engine",
    body="Aurora uses Core Audio AVAudioEngine with a custom mixer node. SwiftUI for UI, UIKit for the player view controller. Minimum iOS 16.",
    area="aurora", importance=4, tags=["audio", "architecture"])

_add(kind="fact", title="Aurora HealthKit integration",
    body="Aurora writes sleep session data to HealthKit with category type sleepAnalysis. Asks authorization on first launch of sleep module.",
    area="aurora", importance=4, tags=["healthkit", "sleep"])

_add(kind="fact", title="Aurora StoreKit 2 subscriptions",
    body="Uses StoreKit 2 AsyncSequence for transaction updates. Two tiers: monthly and yearly. Free tier limited to 3 sessions per day.",
    area="aurora", importance=4, tags=["storekit", "iap"])

_add(kind="fact", title="Aurora minimum iOS version",
    body="Minimum deployment target is iOS 16.0. Dropped iOS 15 support in Q4 to use Observation framework.",
    area="aurora", importance=3, tags=["ios", "deployment"])

# NEAR-DUPLICATE of "Aurora audio engine" above — different wording
_add(kind="fact", title="Aurora tech stack",
    body="Aurora is built on Core Audio (AVAudioEngine) for sound pipeline, SwiftUI for main UI, and UIKit only for the custom player screen. Targets iOS 16+.",
    area="aurora", importance=3, tags=["architecture"])

_add(kind="learning", title="AVAudioEngine background interruption",
    body="When backgrounded during a phone call, AVAudioEngine loses its connection graph. Must reconnect nodes in notification handler or audio dies silently.",
    area="aurora", importance=5, tags=["audio", "gotcha"])

_add(kind="learning", title="HealthKit sleep write permissions",
    body="HealthKit requires HKSharePermissionRequestType to write sleep categories. Read-only permission is NOT enough even for updating existing samples we created.",
    area="aurora", importance=4, tags=["healthkit", "gotcha"])

_add(kind="learning", title="StoreKit 2 transaction finishing",
    body="Must call Transaction.finish() after validating purchase server-side. Otherwise the transaction stays in the queue forever and triggers on every launch.",
    area="aurora", importance=5, tags=["storekit", "gotcha"])

_add(kind="learning", title="Core Audio sample rate mismatch",
    body="If input and output nodes have different sample rates, AVAudioEngine will crash on start. Insert a converter node between them or match rates explicitly.",
    area="aurora", importance=4, tags=["audio"])

_add(kind="note", title="Talked to Alex about mixer refactor",
    body="Alex wants to split the mixer into separate AudioUnit instances so we can add per-layer effects. Agreed to prototype on a branch next week.",
    area="aurora", importance=2, tags=["meeting"])

_add(kind="note", title="Morgan onboarding redesign",
    body="Morgan showed new onboarding mockups. Moving from 4-screen carousel to single scrollable flow. Uses new illustration set.",
    area="aurora", importance=2, tags=["design"])

_add(kind="note", title="Sam tuning reverb preset",
    body="Sam found that reverb wet mix at 0.35 sounds best for the rain preset. Dry preset uses 0.15. Values committed to presets.json.",
    area="aurora", importance=2, tags=["audio"])

_add(kind="skill", title="How to run Aurora tests locally",
    body="cd apps/aurora && xcodebuild test -scheme Aurora -destination 'platform=iOS Simulator,name=iPhone 15'. Needs Xcode 15+.",
    area="aurora", importance=3, tags=["testing"])

_add(kind="skill", title="Aurora TestFlight release",
    body="Bump version in Config.xcconfig, archive via CI (push to release branch), auto-uploads to TestFlight. Approval takes ~20 minutes.",
    area="aurora", importance=3, tags=["release"])

_add(kind="rule", title="Never block main thread in audio callback",
    body="Audio callback runs on a realtime thread. No locks, no malloc, no Objective-C dispatch. Use lock-free ring buffers only.",
    area="aurora", importance=5, tags=["audio", "rule"])

_add(kind="preference", title="Aurora branching convention",
    body="Feature branches are feat/<short-desc>. Bugfix branches are fix/<short-desc>. Release branches are release/<version>.",
    area="aurora", importance=2, tags=["git"])

_add(kind="fact", title="Aurora active user count",
    body="Aurora had 12k monthly actives at end of Q1. Growth is ~8% MoM. Churn is ~4% for paid tier.",
    area="aurora", importance=3, tags=["metrics"])

_add(kind="fact", title="Aurora crash rate",
    body="Crash-free rate is 99.6% on iOS 17, 99.2% on iOS 16. Main crash source is the legacy UIKit player view controller.",
    area="aurora", importance=3, tags=["metrics", "crash"])

# SUPERSEDED pair — old and new deployment target
_add(kind="fact", title="Aurora old deployment target",
    body="Aurora minimum iOS version used to be 15.0 during Q1-Q3 of last year. Bumped to 16.0 in Q4 when we adopted the Observation framework.",
    area="aurora", importance=1, tags=["ios", "history", "outdated"])

_add(kind="fact", title="Aurora audio layer count",
    body="Aurora supports up to 4 simultaneous audio layers. Limit is CPU-bound on older iPhones (SE, 11-series).",
    area="aurora", importance=3, tags=["audio", "limits"])

_add(kind="learning", title="SwiftUI List performance",
    body="SwiftUI List with 500+ items drops frames on iPhone SE. Switched to LazyVStack inside ScrollView, smoother but loses swipe actions.",
    area="aurora", importance=3, tags=["swiftui", "perf"])

_add(kind="learning", title="Combine memory leak with shared subjects",
    body="Assigning a PassthroughSubject to a @Published property creates a retain cycle. Use .assign(to: &$property) with a weak closure instead.",
    area="aurora", importance=4, tags=["combine", "memory"])

_add(kind="skill", title="Aurora localization workflow",
    body="Run fastlane localize — pulls new strings from Lokalise, merges into Localizable.strings per locale, commits. Currently 8 languages.",
    area="aurora", importance=2, tags=["i18n"])

_add(kind="note", title="Aurora 2.1 release postmortem",
    body="2.1 had a regression: subscription status wasn't refreshing after purchase. Hotfix 2.1.1 released 3 days later. Root cause was missing .current check.",
    area="aurora", importance=3, tags=["release", "postmortem"])

_add(kind="fact", title="Aurora subscription price points",
    body="Monthly $4.99, yearly $29.99 (50% savings). Lifetime one-time $79.99. Prices shown via StoreKit, localized automatically.",
    area="aurora", importance=3, tags=["storekit", "pricing"])

_add(kind="conversation", title="Team sync about iPad support",
    body="Team discussed whether to invest in iPad-optimized layout. Decision: postpone to Q3 — not enough demand signal from analytics to justify the work.",
    area="aurora", importance=2, tags=["planning"])

_add(kind="fact", title="Aurora CI provider",
    body="Aurora builds on GitHub Actions using macos-14 runners. Build+test+archive takes ~18 minutes. Fastlane for signing.",
    area="aurora", importance=3, tags=["ci"])

# ─────────────────────────────────────────────────────────────
# EXTRA aurora — edge cases, more dupes
# ─────────────────────────────────────────────────────────────

_add(kind="learning", title="Sandbox StoreKit testing",
    body="StoreKit.configuration file in Xcode enables local IAP testing without sandbox account. Useful for unit tests. Doesn't simulate network errors though.",
    area="aurora", importance=3, tags=["storekit", "testing"])

_add(kind="learning", title="SwiftUI NavigationStack deep link",
    body="NavigationStack with bound path array can deep-link anywhere in the hierarchy. Setting path = [.profile, .settings] programmatically navigates two levels deep.",
    area="aurora", importance=3, tags=["swiftui"])

_add(kind="note", title="Alex found bug in mixer",
    body="Alex found that enabling more than 3 audio layers caused a crash on iPhone SE 2nd gen due to CPU pressure. Added a device-based limit.",
    area="aurora", importance=3, tags=["bug"])

_add(kind="fact", title="Aurora audio presets",
    body="Aurora ships with 24 built-in audio presets: rain, ocean, forest, fire, white noise variants, and guided breathing. New presets monthly.",
    area="aurora", importance=3, tags=["content"])

_add(kind="fact", title="Aurora analytics provider",
    body="Aurora uses Mixtabl for product analytics and Sentry for crash reporting. No PII sent — user id is a hashed device identifier.",
    area="aurora", importance=3, tags=["analytics"])

_add(kind="skill", title="Debug Aurora crash from Sentry",
    body="Open Sentry issue, grab the symbolicated stack trace, check device + iOS version. Reproduce in simulator with same iOS. Most crashes are in the legacy player VC.",
    area="aurora", importance=3, tags=["debugging"])

_add(kind="rule", title="All Aurora PRs need a test",
    body="PRs touching business logic or audio pipeline require at least one new test. UI-only or config changes are exempt. CI blocks PRs missing tests.",
    area="aurora", importance=4, tags=["testing", "rule"])

_add(kind="learning", title="URLSession background downloads",
    body="Background URLSession downloads survive app termination but not device reboot. Register for restoration via UIApplicationDelegate handleEventsForBackgroundURLSession.",
    area="aurora", importance=3, tags=["networking"])

_add(kind="preference", title="Aurora code review style",
    body="Code review: be specific, cite exact lines, suggest alternatives not just criticism. Approve as soon as concerns addressed, don't nitpick formatting.",
    area="aurora", importance=2, tags=["review"])

_add(kind="fact", title="Aurora crash grouping",
    body="Sentry crash grouping set to 'frame in app' + exception type. Prevents one user's weird crash from dominating alerts with identical noise.",
    area="aurora", importance=2, tags=["sentry"])
