import hashlib,json,re,sqlite3,sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
RELEASES = ROOT / "src-tauri/crates/codeterm-plan/tests/fixtures/releases"
SOURCE = ROOT / "src-tauri/crates/codeterm-plan/src/service_plan"
BOOTSTRAP_SEAL = "963d85b0d97183163d6a9048bd9e644088efc6eaf7f7775896de693c59bfdbf6"


def encoded(value):
    return json.dumps(value, ensure_ascii=False, separators=(",", ":")).encode()


def digest(data):
    return hashlib.sha256(data).hexdigest()


def read_json(name):
    return json.loads((RELEASES / name).read_text(encoding="utf-8"))


def constant(source, name):
    match = re.search(r"const " + name + r": &str = r#\"(.*?)\"#;", source, re.S)
    if not match:
        raise ValueError(f"Cannot resolve persisted schema constant {name}")
    return match[1]


def schema(connection):
    tokens = r"'(?:''|[^'])*'|\"(?:\"\"|[^\"])*\"|`(?:``|[^`])*`|\[(?:\]\]|[^\]])*\]|[\w$]+|[^\s]"
    return [[kind, name, table, "\0".join(re.findall(tokens, sql)) if sql else None]
            for kind, name, table, sql in connection.execute(
                "SELECT type,name,tbl_name,sql FROM sqlite_schema "
                "WHERE name NOT GLOB 'sqlite_*' ORDER BY type,name")]


def bootstrap(manifest, contract):
    return {"fixtures": manifest["fixtures"][:3],
            "equivalent_releases": manifest["equivalent_releases"][:1],
            "records_sha256": manifest["records_sha256"],
            "migration": contract["migrations"][0]}


def check():
    manifest = read_json("manifest.json")
    contract = read_json("contract.json")
    if digest(encoded(bootstrap(manifest, contract))) != BOOTSTRAP_SEAL:
        raise ValueError("Published plan fixtures or the initial migration contract were edited")
    if digest((RELEASES / "records.json").read_bytes()) != manifest["records_sha256"]:
        raise ValueError("Published logical records were edited")
    fixtures = {}
    for entry in manifest["fixtures"]:
        file = entry["file"]
        if Path(file).name != file or file in fixtures:
            raise ValueError(f"Invalid or duplicate fixture: {file}")
        data = (RELEASES / file).read_bytes()
        if digest(data) != entry["sha256"]:
            raise ValueError(f"Published fixture was edited: {file}")
        with sqlite3.connect(":memory:") as connection:
            connection.executescript(data.decode("utf-8"))
            version = connection.execute("PRAGMA user_version").fetchone()[0]
            if version != entry["user_version"] or digest(encoded(schema(connection))) != entry["schema_fingerprint"]:
                raise ValueError(f"Fixture schema contract mismatch: {file}")
        fixtures[file] = entry
    if {file.name for file in RELEASES.glob("*.sql")} != set(fixtures):
        raise ValueError("Every release fixture must have a manifest entry")
    previous = None
    for migration in contract["migrations"]:
        if previous and migration["user_version"] <= previous["user_version"]:
            raise ValueError("New schema changes need a new versioned migration entry")
        entry = fixtures[migration["fixture"]]
        if any(migration[key] != entry[key] for key in ["schema_fingerprint", "user_version"]):
            raise ValueError("Migration entry must match its new release fixture")
        previous = migration
    target = contract["target"]
    if target != {key: previous[key] for key in target}:
        raise ValueError("Target must identify the latest registered migration")
    ddl = ""
    for entry in contract["ddl_sources"]:
        ddl += constant((SOURCE / entry["file"]).read_text(encoding="utf-8"), entry["constant"])
    with sqlite3.connect(":memory:") as connection:
        connection.executescript(ddl)
        if digest(encoded(schema(connection))) != target["schema_fingerprint"]:
            raise ValueError("Persisted plan schema changed without a new fixture/migration entry")
    source = (SOURCE / "store_db.rs").read_text(encoding="utf-8")
    creation = re.search(r"pub\(super\) fn create_schema\(.*?\n}", source, re.S)[0]
    if digest(encoded(re.findall(r"\w+|[^\s]", creation))) != previous["creation_fingerprint"]:
        raise ValueError("Schema creation wiring changed without a migration entry")
    version = int(re.search(r"const SCHEMA_VERSION: i64 = (\d+);", source)[1])
    if version != target["user_version"]:
        raise ValueError("Plan user_version differs from the migration contract")
    print(f"Plan schema contract: {len(fixtures)} immutable fixtures; target v{version}; {target['schema_fingerprint']}")


if __name__ == "__main__":
    try:
        check()
    except (ValueError, KeyError, OSError, sqlite3.Error) as error:
        print(f"Plan schema contract FAILED: {error}", file=sys.stderr)
        sys.exit(1)
