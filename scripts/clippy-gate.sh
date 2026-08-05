#!/usr/bin/env bash
# Single source of truth for the clippy gate — called by .githooks/pre-push
# (integration pushes) and .github/workflows/tests.yml (CI linux job).
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

CLIPPY_ALLOW=(
    # Visibility debt is separate from reachability; keep it explicit until that API audit lands.
    -A private-interfaces
    -A clippy::empty_line_after_doc_comments
    -A clippy::duplicated_attributes
    -A clippy::manual_abs_diff
    -A clippy::needless_range_loop
    -A clippy::redundant_closure
    -A clippy::new_without_default
    -A clippy::question_mark
    -A clippy::unnecessary_map_or
    -A clippy::single_char_add_str
    -A clippy::collapsible_else_if
    -A clippy::field_reassign_with_default
    -A clippy::items_after_test_module
    -A clippy::too_many_arguments
    -A clippy::type_complexity
    -A clippy::unnecessary_get_then_check
    -A clippy::ptr_arg
    -A clippy::needless_as_bytes
    -A clippy::single_element_loop
    -A clippy::bool_assert_comparison
    -A clippy::manual_repeat_n
    -A clippy::len_zero
    -A clippy::useless_vec
    -A clippy::needless_borrows_for_generic_args
    -A clippy::manual_clamp
    -A clippy::upper_case_acronyms
    -A clippy::needless_question_mark
    -A clippy::implicit_saturating_sub
    -A clippy::needless_borrow
    -A clippy::derivable_impls
    -A clippy::unnecessary_cast
    -A clippy::trim_split_whitespace
    -A clippy::should_implement_trait
    -A clippy::collapsible_str_replace
    -A clippy::missing_transmute_annotations
    -A clippy::doc_lazy_continuation
    -A clippy::manual_flatten
    -A clippy::for_kv_map
    -A clippy::manual_div_ceil
    -A clippy::doc_overindented_list_items
    -A clippy::useless_conversion
    -A clippy::explicit_auto_deref
    -A clippy::while_let_loop
    -A clippy::map_identity
    -A clippy::nonminimal_bool
    -A clippy::manual_pattern_char_comparison
    -A clippy::while_let_on_iterator
    -A clippy::unnecessary_unwrap
    -A clippy::single_match
    -A clippy::unwrap_or_default
    -A clippy::manual_split_once
    -A clippy::wrong_self_convention
    -A clippy::io_other_error
    -A clippy::manual_is_multiple_of
    -A clippy::clone_on_copy
    -A clippy::large_enum_variant
    -A clippy::manual_contains
    -A clippy::map_flatten
    -A clippy::print_literal
    -A clippy::cloned_ref_to_slice_refs
    -A clippy::filter_next
    -A clippy::needless_return
    -A clippy::collapsible_match
    # Pre-existing lints surfaced by newer stable clippy (>=1.95); toolchain is
    # pinned in rust-toolchain.toml, these cover the baseline it flags.
    -A clippy::unnecessary_sort_by
    -A unused_unsafe
    # Sibling of the allowed large_enum_variant: boxing every returning signature
    # is more churn than the by-value cost is worth for these error types.
    -A clippy::result_large_err
)

run_clippy_gate() {
    local manifest="$1"
    shift
    cargo clippy --manifest-path "$manifest" --workspace "$@" -- -D warnings "${CLIPPY_ALLOW[@]}" || return $?
    cargo clippy --manifest-path "$manifest" --workspace --all-targets -- -D warnings "${CLIPPY_ALLOW[@]}" || return $?
}

run_clippy_gate src-tauri/Cargo.toml --lib --bins --examples || exit $?
run_clippy_gate cli/Cargo.toml --bins --examples || exit $?
