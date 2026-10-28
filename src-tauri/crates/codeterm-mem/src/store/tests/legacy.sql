

        CREATE TABLE IF NOT EXISTS mem_items (
            id            TEXT PRIMARY KEY,
            uid           TEXT NOT NULL UNIQUE,
            kind          TEXT NOT NULL,
            scope         TEXT NOT NULL,
            title         TEXT NOT NULL,
            summary       TEXT,
            area          TEXT,
            tags_json     TEXT NOT NULL DEFAULT '[]',
            body_inline   TEXT,
            body_blob_path TEXT,
            source        TEXT,
            importance    REAL NOT NULL DEFAULT 3.0,
            created_at    TEXT NOT NULL,
            updated_at    TEXT NOT NULL,
            accessed_at   TEXT,
            access_count  INTEGER NOT NULL DEFAULT 0,
            recurrence_count INTEGER NOT NULL DEFAULT 0
        );

        CREATE INDEX IF NOT EXISTS idx_mem_kind ON mem_items(kind);
        CREATE INDEX IF NOT EXISTS idx_mem_scope ON mem_items(scope);
        CREATE INDEX IF NOT EXISTS idx_mem_area ON mem_items(area);
        CREATE INDEX IF NOT EXISTS idx_mem_importance ON mem_items(importance DESC);
        CREATE INDEX IF NOT EXISTS idx_mem_created ON mem_items(created_at DESC);

        CREATE VIRTUAL TABLE IF NOT EXISTS mem_fts USING fts5(
            mem_id UNINDEXED,
            title,
            summary,
            body,
            tags,
            tokenize = 'unicode61'
        );

        CREATE TABLE IF NOT EXISTS entities (
            id            TEXT PRIMARY KEY,
            entity_type   TEXT NOT NULL,
            scope         TEXT NOT NULL,
            properties_json TEXT NOT NULL DEFAULT '{}',
            created_at    TEXT NOT NULL,
            updated_at    TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_entities_type ON entities(entity_type);

        CREATE TABLE IF NOT EXISTS relations (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            from_id         TEXT NOT NULL,
            relation_type   TEXT NOT NULL,
            to_id           TEXT NOT NULL,
            scope           TEXT NOT NULL,
            valid_from      TEXT NOT NULL,
            valid_to        TEXT,
            confidence      REAL NOT NULL DEFAULT 1.0,
            properties_json TEXT NOT NULL DEFAULT '{}',
            created_at      TEXT NOT NULL,
            UNIQUE(from_id, relation_type, to_id, valid_from)
        );
        CREATE INDEX IF NOT EXISTS idx_relations_from ON relations(from_id);
        CREATE INDEX IF NOT EXISTS idx_relations_to ON relations(to_id);
        CREATE INDEX IF NOT EXISTS idx_relations_type ON relations(relation_type);
        CREATE INDEX IF NOT EXISTS idx_relations_valid ON relations(valid_from, valid_to);

        CREATE TABLE IF NOT EXISTS entity_aliases (
            alias_id     TEXT PRIMARY KEY,
            canonical_id TEXT NOT NULL,
            created_at   TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_aliases_canonical ON entity_aliases(canonical_id);

        CREATE TABLE IF NOT EXISTS mem_precompact_purges (
            item_id TEXT PRIMARY KEY,
            history_rows INTEGER NOT NULL,
            history_body_bytes INTEGER NOT NULL,
            purged_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS mem_precompact_purge_blobs (
            item_id TEXT NOT NULL,
            path TEXT NOT NULL,
            body_bytes INTEGER NOT NULL,
            PRIMARY KEY (item_id, path)
        );

        CREATE TABLE IF NOT EXISTS mem_item_history (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            item_id         TEXT NOT NULL,
            changed_at      TEXT NOT NULL,
            changed_by      TEXT NOT NULL,
            change_type     TEXT NOT NULL,
            old_title       TEXT,
            new_title       TEXT,
            old_body        TEXT,
            new_body        TEXT,
            old_importance  REAL,
            new_importance  REAL,
            old_kind        TEXT,
            new_kind        TEXT,
            reason          TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_history_item ON mem_item_history(item_id, changed_at DESC);
        CREATE INDEX IF NOT EXISTS idx_history_changed_by ON mem_item_history(changed_by);
        

        CREATE TABLE IF NOT EXISTS graph_pinned_positions (
            mem_id      TEXT PRIMARY KEY,
            x           REAL NOT NULL,
            y           REAL NOT NULL,
            updated_at  INTEGER NOT NULL,
            FOREIGN KEY (mem_id) REFERENCES mem_items(id) ON DELETE CASCADE
        );
        
ALTER TABLE mem_items ADD COLUMN source_path TEXT;
ALTER TABLE mem_items ADD COLUMN source_size INTEGER;
ALTER TABLE mem_items ADD COLUMN source_mtime TEXT;
ALTER TABLE mem_items ADD COLUMN evidence_count INTEGER NOT NULL DEFAULT 1;
ALTER TABLE mem_items ADD COLUMN false_fire_count INTEGER NOT NULL DEFAULT 0;
ALTER TABLE mem_items ADD COLUMN promotion_threshold INTEGER;
ALTER TABLE mem_items ADD COLUMN is_pinned_wakeup INTEGER NOT NULL DEFAULT 0;
ALTER TABLE mem_items ADD COLUMN seed_fingerprint TEXT;
ALTER TABLE relations ADD COLUMN reason TEXT;
INSERT INTO mem_items(id,uid,kind,scope,title,body_inline,created_at,updated_at) VALUES('legacy','legacy','fact','global','Legacy','prior body','now','now');
INSERT INTO mem_item_history(item_id,changed_at,changed_by,change_type,old_body) VALUES('legacy','now','user','update','prior history');
