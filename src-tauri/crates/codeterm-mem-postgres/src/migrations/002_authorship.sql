ALTER TABLE mem_items
    ADD COLUMN author_kind TEXT NOT NULL DEFAULT 'system' CHECK(author_kind IN ('owner','neighbor','system')),
    ADD COLUMN author_uid TEXT,
    ADD COLUMN author_friendship TEXT,
    ADD COLUMN author_label TEXT DEFAULT 'unscoped',
    ADD CONSTRAINT neighbor_identity CHECK(author_kind != 'neighbor' OR (author_uid IS NOT NULL AND author_friendship IS NOT NULL)),
    ADD CONSTRAINT neighbor_instructions CHECK(author_kind != 'neighbor' OR (kind NOT IN ('rule','preference','skill') AND is_pinned_wakeup=0));
ALTER TABLE mem_item_history
    ADD COLUMN author_kind TEXT NOT NULL DEFAULT 'system' CHECK(author_kind IN ('owner','neighbor','system')),
    ADD COLUMN author_uid TEXT,
    ADD COLUMN author_friendship TEXT,
    ADD COLUMN author_label TEXT DEFAULT 'unscoped',
    ADD CONSTRAINT neighbor_history_identity CHECK(author_kind != 'neighbor' OR (author_uid IS NOT NULL AND author_friendship IS NOT NULL));

CREATE FUNCTION guard_neighbor_update() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF OLD.author_kind='neighbor' THEN
        IF NEW.kind IN ('rule','preference','skill') OR NEW.is_pinned_wakeup != 0 THEN
            RAISE EXCEPTION 'neighbor content cannot become instructions or be pinned' USING ERRCODE='23514';
        END IF;
        IF OLD.kind='learning' AND NEW.kind='fact' THEN
            RAISE EXCEPTION 'neighbor learning cannot be promoted' USING ERRCODE='23514';
        END IF;
    END IF;
    RETURN NEW;
END;
$$;
CREATE TRIGGER mem_neighbor_update_guard BEFORE UPDATE ON mem_items FOR EACH ROW EXECUTE FUNCTION guard_neighbor_update();
