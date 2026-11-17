ALTER TABLE mem_items ADD COLUMN neighbor_skill_authorized INTEGER NOT NULL DEFAULT 0
    CHECK(neighbor_skill_authorized IN (0,1));

DROP TRIGGER mem_neighbor_insert_guard;
CREATE TRIGGER mem_neighbor_insert_guard BEFORE INSERT ON mem_items
WHEN NEW.author_kind='neighbor' AND
    (NEW.kind IN ('rule','preference','secret') OR NEW.is_pinned_wakeup != 0 OR
     (NEW.kind='skill' AND NEW.neighbor_skill_authorized != 1))
BEGIN SELECT RAISE(ABORT, 'neighbor content cannot become instructions or be pinned'); END;

DROP TRIGGER mem_neighbor_update_guard;
CREATE TRIGGER mem_neighbor_update_guard BEFORE UPDATE ON mem_items
WHEN (OLD.author_kind='neighbor' OR NEW.author_kind='neighbor') AND
    (NEW.kind IN ('rule','preference','secret') OR NEW.is_pinned_wakeup != 0 OR
     (NEW.kind='skill' AND (NEW.neighbor_skill_authorized != 1 OR OLD.kind != 'skill')))
BEGIN SELECT RAISE(ABORT, 'neighbor content cannot become instructions or be pinned'); END;
