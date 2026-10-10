# Local tauri-runtime-wry 2.10.1 ownership patch

This directory retains the exact crates.io 2.10.1 source, package version and
dependencies. Upstream revision is `9b17a7aeae9a83222ffe829aa4e2d8a5ba6bed8c`
(`.cargo_vcs_info.json`); the source archive SHA-256 is
`e11ea2e6f801d275fdd890d6c9603736012742a1c33b96d0db788c9cdebf7f9e`.
Original Apache-2.0/MIT licenses, `Cargo.toml.orig`, and `UPSTREAM_README.md` remain.
The application selects this source through its Cargo patch and lockfile.

The exact-image Windows dump for build `0856019648f2d82ccb8b9d294e594f3b7e895a8b`
shows Rc count overflow on thread 26644 (`main`) during the Tauri IPC
Webview/AppHandle/Context/Tao target clone chain. Saved `usize::MAX` is observed;
the earlier count-corrupting race or use-after-free remains unproved.

- [Ownership issue #15408](https://github.com/tauri-apps/tauri/issues/15408).
- [Exact-image evidence report](https://github.com/tauri-apps/tauri/issues/15408#issuecomment-6096821796).
- [Related ownership proposal #15411](https://github.com/tauri-apps/tauri/pull/15411).
- [Earlier defect #10001](https://github.com/tauri-apps/tauri/issues/10001) involved
  a different Rc field; it does not prove identical causality.

## Ownership and teardown

`Context.main_thread` shares one `Arc<OwnerThread<DispatcherMainThreadContext>>`.
The native payload is constructed once on the event-loop thread. Context and
WryHandle clones/drops change atomic shared counts, without cloning/dropping the
embedded native Tao target. Borrowing through the holder preserves every upstream
reference-read method and signature, including Linux display handles and monitors.
No weak upgrade, temporary borrow, display connection workaround, or API change
is introduced. Holder Debug prints thread metadata without native dereferences.

`OwnerThread` records the creation thread and stores its payload in `ManuallyDrop`.
Final shared release on that thread destroys native data there. Final release on
another thread deliberately leaks the payload instead of destroying thread-affine
data. This also covers late worker handles after event-loop exit without posting
work to a dead event loop. Worker references keep borrowed payload storage alive
for their existing handle lifetime. The intentional late-drop leak is the chosen
teardown tradeoff, not an automatic cleanup guarantee.

This changes ownership only. Existing upstream unsafe Send/Sync contracts and
off-thread reference reads remain outside scope; this patch does not claim to
make those reads safe, prevent explicit native connection close, or establish the
historic count corruption origin. Native clone/drop ownership protection and the
independent panic-hook diagnostics are distinct repairs.

## Focused regression and verification

The application's test-only path import compiles the actual production helper and
`src/owner_thread/tests.rs`, using an Rc-bearing native-style payload. Workers only
clone/drop shared holders or format metadata. Tests cover concurrent holder
clone/drop with owner Rc operations, late worker final release without native
destruction, a worker returning the final handle for owner-thread destruction, and final
worker release during controlled unwind.
The fixture mirrors the existing native context Send/Sync boundary.

```bash
bash ./build.sh test-one app windows_rc_ownership_regression
```

Expected: exactly six executed passing cases, no ignored/failures, exit 0.
A controlled source red check can remove the helper's owner-thread Drop condition:
the late-worker case then observes a native destructor on the worker and fails.
Restore the condition for green. This tests the destructor policy without racing
native Rc operations. It does not replace the owner-run DEV baseline/fixed invoke
workload or prove historical corruption. Worker source checks do not compile/run
Rust. Independent ownership review and Windows/macOS/Linux canonical gates are
required; no execution result is claimed here.

## Removal and rollback

Remove the patch when adopting an upstream **released** equivalent ownership fix
with accepted thread-affine final-drop behavior and successful focused/live proof.
A merged PR alone is insufficient. Rollback reverts the primary vendor/Cargo
patch/lockfile/test-import commit, preserving separate diagnostic ancestry and
all crash evidence.

## Paired deterministic Context-field oracle

`ContextOwnership<C>` isolates the one ownership field used by derived Context
Clone. The shared case uses the real production `Arc<OwnerThread<NativeOwner>>`;
the counterfactual uses the pinned by-value, derived-Clone payload shape. A single
worker clones once, signals completion, waits for release, drops the clone and
returns the original for owner disposal. Rc count observations occur only between
those acknowledged phases; there is no unsynchronized native count concurrency.
The same unchanged-count assertion passes for shared ownership (2 -> 2 -> 2)
and fails for by-value ownership (2 -> 3 -> 2). The normal counterfactual test
catches that expected assertion failure; it is not itself an uncaught red run.

The source-only patch changes only the shared test's payload wrapper to by-value,
so canonical can expose the same invariant as an ordinary failing test:

```bash
git apply --ignore-space-change src-tauri/vendor/tauri-runtime-wry/src/owner_thread/pinned-by-value-red.patch
bash ./build.sh test-one app windows_rc_ownership_regression_context_field_shared_count_invariant
```

Expected red: one executed failing case, count 3 versus 2, nonzero gate exit.
Record that result, then restore before green (even if the red command fails):

```bash
git apply -R --ignore-space-change src-tauri/vendor/tauri-runtime-wry/src/owner_thread/pinned-by-value-red.patch
bash ./build.sh test-one app windows_rc_ownership_regression_context_field_shared_count_invariant
bash ./build.sh test-one app windows_rc_ownership_regression_context_field_by_value_count_invariant_fails
bash ./build.sh test-one app windows_rc_ownership_regression
```

Expected green: one passing case in each focused command and six passing cases in
the complete selector, zero ignored/failures. These commands have not run in the
Worker lane. The patch modifies test coverage only, never production ownership.

This is a production-holder/field-boundary oracle, not full native Context
construction. Real `DispatcherMainThreadContext` requires Tao
`EventLoopWindowTarget<Message<T>>`, obtained from a platform event loop; full
Context additionally needs its live proxy/window stores. Fabricating those values
would require invalid native state or runtime setup. The pure fixture substitutes
an Rc-bearing target side and models the exact changed field/derived Clone shape;
it does not instantiate native Tao/COM/GDK state or prove the original crash race.
Owner judgment, immutable source review and DEV workload evidence remain required.
