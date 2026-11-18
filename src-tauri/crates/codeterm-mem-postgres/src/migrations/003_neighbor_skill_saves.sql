ALTER TABLE mem_items
    ADD COLUMN neighbor_skill_authorized BIGINT NOT NULL DEFAULT 0 CHECK(neighbor_skill_authorized IN (0,1)),
    DROP CONSTRAINT neighbor_instructions,
    ADD CONSTRAINT neighbor_instructions CHECK(author_kind != 'neighbor' OR
        (kind NOT IN ('rule','preference','secret') AND is_pinned_wakeup=0 AND
         (kind != 'skill' OR neighbor_skill_authorized=1)));

CREATE OR REPLACE FUNCTION guard_neighbor_update() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF OLD.author_kind='neighbor' OR NEW.author_kind='neighbor' THEN
        IF NEW.kind IN ('rule','preference','secret') OR NEW.is_pinned_wakeup != 0 OR
           (NEW.kind='skill' AND (NEW.neighbor_skill_authorized != 1 OR OLD.kind != 'skill')) THEN
            RAISE EXCEPTION 'neighbor content cannot become instructions or be pinned' USING ERRCODE='23514';
        END IF;
        IF OLD.author_kind='neighbor' AND OLD.kind='learning' AND NEW.kind='fact' THEN
            RAISE EXCEPTION 'neighbor learning cannot be promoted' USING ERRCODE='23514';
        END IF;
    END IF;
    RETURN NEW;
END;
$$;
