import hashlib,json,re,sqlite3,sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
RELEASES = ROOT / "src-tauri/crates/codeterm-plan/tests/fixtures/releases"
SOURCE = ROOT / "src-tauri/crates/codeterm-plan/src/service_plan"
BOOTSTRAP_SEAL = "963d85b0d97183163d6a9048bd9e644088efc6eaf7f7775896de693c59bfdbf6"

STEPS_SEAL = "80f2b9c4647fbcbf6d68d223f49371cac32193c767432dc15306fd123e9154b0"


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
            if connection.execute("PRAGMA application_id").fetchone()[0] != entry.get("application_id", 0):
                raise ValueError(f"Fixture application identity mismatch: {file}")
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
    registered = contract["registered_steps"]
    if digest(encoded(registered[:4])) != STEPS_SEAL:
        raise ValueError("Published numbered migration steps or schema assertions were edited")
    migration_source = (SOURCE / "store_db/migrations.rs").read_text(encoding="utf-8")
    declarations = re.findall(r'version: (\d+),\s*sql: include_str!\("migrations/([^"/]+)"\),\s*schema: include_str!\("migrations/schemas/([^"/]+)"\)', migration_source)
    if declarations != [(str(step["version"]), step["step"], step["schema"]) for step in registered]:
        raise ValueError("Every schema version needs a registered executable migration step")
    if [step["version"] for step in registered] != list(range(1, len(registered) + 1)):
        raise ValueError("Migration versions must be contiguous")
    if "SCHEMA_VERSION: i64 = migrations::SCHEMA_VERSION;" not in source or "SCHEMA_VERSION: i64 = STEPS[STEPS.len() - 1].version;" not in migration_source:
        raise ValueError("SCHEMA_VERSION must derive from the last registered step")
    registrations = re.findall(r"    Step \{(.*?)\n    \}", migration_source, re.S)
    if len(registrations) != len(registered) or any(digest(encoded(re.findall(r"\w+|[^\s]", declaration))) != step["registration_sha256"] for declaration, step in zip(registrations, registered)):
        raise ValueError("Migration registration or constraint preconditions changed without a new step")
    steps_directory = SOURCE / "store_db/migrations"
    for snapshot_name, fixture_name in [("001.sql", "v1.18.6.sql"), ("published-v1.18.7.sql", "v1.18.7.sql")]:
        if (steps_directory / "schemas" / snapshot_name).read_text(encoding="utf-8") != (RELEASES / fixture_name).read_text(encoding="utf-8").split("PRAGMA user_version")[0]:
            raise ValueError("Published source-schema assertions differ from immutable release inputs")
    if {file.name for file in steps_directory.glob("*.sql")} != {step["step"] for step in registered}:
        raise ValueError("Unregistered migration SQL")
    with sqlite3.connect(":memory:") as connection:
        for step in registered:
            sql = (steps_directory / step["step"]).read_bytes()
            snapshot = (steps_directory / "schemas" / step["schema"]).read_bytes()
            if digest(sql) != step["sha256"] or digest(snapshot) != step["schema_sha256"]:
                raise ValueError("Migration step or schema assertion hash mismatch")
            connection.executescript(sql.decode())
            with sqlite3.connect(":memory:") as expected:
                expected.executescript(snapshot.decode())
                if schema(connection) != schema(expected) or connection.execute("PRAGMA application_id").fetchone() != expected.execute("PRAGMA application_id").fetchone():
                    raise ValueError("Registered step does not produce its exact schema assertion")
            if step["version"] > 1:
                entry = next((entry for entry in contract["migrations"] if entry["user_version"] == step["version"]), None)
                if not entry or entry.get("step") != step["step"] or entry["schema_fingerprint"] != digest(encoded(schema(connection))):
                    raise ValueError("Each executable migration needs its own fixture and contract entry")
        if digest(encoded(schema(connection))) != target["schema_fingerprint"]:
            raise ValueError("Persisted schema changed without a new registered migration step")
    for fixture in fixtures.values():
        if fixture["user_version"] == 0:
            continue
        with sqlite3.connect(":memory:") as connection:
            connection.executescript((RELEASES / fixture["file"]).read_text(encoding="utf-8"))
            for step in registered:
                if step["version"] > fixture["user_version"]:
                    connection.executescript((steps_directory / step["step"]).read_text(encoding="utf-8"))
            if digest(encoded(schema(connection))) != target["schema_fingerprint"] or connection.execute("PRAGMA application_id").fetchone()[0] != fixtures[target["fixture"]].get("application_id", 0):
                raise ValueError(f"Forward replay did not converge: {fixture['file']}")
            if connection.execute("PRAGMA foreign_key_check").fetchone():
                raise ValueError(f"Forward replay violated foreign keys: {fixture['file']}")
    version = registered[-1]["version"]
    if version != target["user_version"]:
        raise ValueError("Plan target version differs from the final registered step")
    print(f"Plan schema contract: {len(fixtures)} immutable fixtures; target v{version}; {target['schema_fingerprint']}")


if __name__ == "__main__":
    try:
        check()
    except (ValueError, KeyError, OSError, sqlite3.Error) as error:
        print(f"Plan schema contract FAILED: {error}", file=sys.stderr)
        sys.exit(1)
