CREATE TRIGGER mem_neighbor_insert_guard BEFORE INSERT ON mem_items
WHEN NEW.author_kind='neighbor' AND
    (NEW.kind IN ('rule','preference','skill') OR NEW.is_pinned_wakeup != 0)
BEGIN SELECT RAISE(ABORT, 'neighbor content cannot become instructions or be pinned'); END;

CREATE TRIGGER mem_neighbor_update_guard BEFORE UPDATE ON mem_items
WHEN (OLD.author_kind='neighbor' OR NEW.author_kind='neighbor') AND
    (NEW.kind IN ('rule','preference','skill') OR NEW.is_pinned_wakeup != 0)
BEGIN SELECT RAISE(ABORT, 'neighbor content cannot become instructions or be pinned'); END;

CREATE TRIGGER mem_neighbor_promote_guard BEFORE UPDATE OF kind ON mem_items
WHEN OLD.author_kind='neighbor' AND OLD.kind='learning' AND NEW.kind='fact'
BEGIN SELECT RAISE(ABORT, 'neighbor learning cannot be promoted'); END;

CREATE TRIGGER mem_author_insert_guard BEFORE INSERT ON mem_items
WHEN NEW.author_kind='neighbor' AND (NEW.author_uid IS NULL OR NEW.author_friendship IS NULL)
BEGIN SELECT RAISE(ABORT, 'neighbor author requires account and friendship'); END;

CREATE TRIGGER mem_author_update_guard BEFORE UPDATE ON mem_items
WHEN NEW.author_kind='neighbor' AND (NEW.author_uid IS NULL OR NEW.author_friendship IS NULL)
BEGIN SELECT RAISE(ABORT, 'neighbor author requires account and friendship'); END;

CREATE TRIGGER mem_history_author_guard BEFORE INSERT ON mem_item_history
WHEN NEW.author_kind='neighbor' AND (NEW.author_uid IS NULL OR NEW.author_friendship IS NULL)
BEGIN SELECT RAISE(ABORT, 'neighbor history author requires account and friendship'); END;
