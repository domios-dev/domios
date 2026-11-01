
    (CASE WHEN pg_input_is_valid(m.created_at,'timestamp with time zone') THEN m.created_at::timestamptz END) < $1::text::timestamptz
    AND COALESCE(m.is_pinned_wakeup, 0) = 0
    AND m.kind NOT IN ('skill', 'rule', 'preference', 'archived')
    AND m.importance < 4.0
    AND (CASE WHEN pg_input_is_valid(m.updated_at,'timestamp with time zone') THEN m.updated_at::timestamptz END) < $1::text::timestamptz
    AND (m.accessed_at IS NULL OR (CASE WHEN pg_input_is_valid(m.accessed_at,'timestamp with time zone') THEN m.accessed_at::timestamptz END) < $1::text::timestamptz)
    AND NOT EXISTS (SELECT 1 FROM mem_item_history h
        WHERE h.item_id = m.id AND (CASE WHEN pg_input_is_valid(h.changed_at,'timestamp with time zone') THEN h.changed_at::timestamptz END) >= $1::text::timestamptz)
    AND NOT EXISTS (SELECT 1 FROM relations r WHERE r.from_id = m.id
        AND ((CASE WHEN pg_input_is_valid(r.valid_from,'timestamp with time zone') THEN r.valid_from::timestamptz END) >= $1::text::timestamptz OR (CASE WHEN pg_input_is_valid(r.created_at,'timestamp with time zone') THEN r.created_at::timestamptz END) >= $1::text::timestamptz
             OR (CASE WHEN pg_input_is_valid(r.valid_to,'timestamp with time zone') THEN r.valid_to::timestamptz END) >= $1::text::timestamptz))
    AND NOT EXISTS (SELECT 1 FROM relations r WHERE r.to_id = m.id
        AND ((CASE WHEN pg_input_is_valid(r.valid_from,'timestamp with time zone') THEN r.valid_from::timestamptz END) >= $1::text::timestamptz OR (CASE WHEN pg_input_is_valid(r.created_at,'timestamp with time zone') THEN r.created_at::timestamptz END) >= $1::text::timestamptz
             OR (CASE WHEN pg_input_is_valid(r.valid_to,'timestamp with time zone') THEN r.valid_to::timestamptz END) >= $1::text::timestamptz))
    AND NOT EXISTS (SELECT 1 FROM relations r JOIN mem_items n ON n.id = r.to_id
        WHERE r.from_id = m.id AND r.valid_to IS NULL
          AND ((CASE WHEN pg_input_is_valid(n.updated_at,'timestamp with time zone') THEN n.updated_at::timestamptz END) >= $1::text::timestamptz OR (CASE WHEN pg_input_is_valid(n.accessed_at,'timestamp with time zone') THEN n.accessed_at::timestamptz END) >= $1::text::timestamptz))
    AND NOT EXISTS (SELECT 1 FROM relations r JOIN mem_items n ON n.id = r.from_id
        WHERE r.to_id = m.id AND r.valid_to IS NULL
          AND ((CASE WHEN pg_input_is_valid(n.updated_at,'timestamp with time zone') THEN n.updated_at::timestamptz END) >= $1::text::timestamptz OR (CASE WHEN pg_input_is_valid(n.accessed_at,'timestamp with time zone') THEN n.accessed_at::timestamptz END) >= $1::text::timestamptz))
    AND NOT EXISTS (SELECT 1 FROM relations r JOIN entities e ON e.id = r.to_id
        WHERE r.from_id = m.id AND r.valid_to IS NULL AND (CASE WHEN pg_input_is_valid(e.updated_at,'timestamp with time zone') THEN e.updated_at::timestamptz END) >= $1::text::timestamptz)
    AND NOT EXISTS (SELECT 1 FROM relations r JOIN entities e ON e.id = r.from_id
        WHERE r.to_id = m.id AND r.valid_to IS NULL AND (CASE WHEN pg_input_is_valid(e.updated_at,'timestamp with time zone') THEN e.updated_at::timestamptz END) >= $1::text::timestamptz)