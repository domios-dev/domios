from __future__ import annotations

from hashlib import sha256


MAX_SLUG_LENGTH = 60
MAX_TITLE_BYTES = 200


def title_slug(title: str) -> str:
    output = []
    last_underscore = False
    for character in title:
        if character.isascii() and character.isalnum():
            output.append(character.lower())
            last_underscore = False
        elif output and not last_underscore:
            output.append("_")
            last_underscore = True
    slug = "".join(output).rstrip("_")
    if not slug:
        slug = "u" + sha256(title.encode("utf-8")).hexdigest()[:8]
    return slug[:MAX_SLUG_LENGTH].rstrip("_")


def proposed_item_id(kind: str, title: str) -> str:
    return f"{kind}_{title_slug(title)}"


def disambiguate_title(
    title: str,
    source_id: str,
    kind: str,
    occupied_ids: set[str],
    *,
    force_suffix: bool = False,
    own_id: str | None = None,
) -> str:
    occupied = set(occupied_ids)
    if own_id is not None:
        occupied.discard(own_id)
    base_id = proposed_item_id(kind, title)
    if not force_suffix and base_id not in occupied:
        return title

    digest = sha256(source_id.encode("utf-8")).hexdigest()
    for digest_length in range(8, 57, 4):
        suffix = digest[:digest_length]
        suffix_slug = f"src_{suffix}"
        max_prefix_slug = MAX_SLUG_LENGTH - len(suffix_slug) - 1
        marker = f" [src:{suffix}]"
        prefix = title

        def make_candidate(value: str) -> str:
            stem = value.rstrip()
            if value != title and stem:
                stem += "…"
            return f"{stem}{marker}"

        while prefix and (
            len(title_slug(prefix)) > max_prefix_slug
            or len(make_candidate(prefix).encode("utf-8")) > MAX_TITLE_BYTES
        ):
            prefix = prefix[:-1]
        candidate = make_candidate(prefix)
        if proposed_item_id(kind, candidate) not in occupied:
            return candidate
    raise ValueError(f"could not disambiguate title for source id {source_id}")
