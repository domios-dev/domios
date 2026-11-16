use crate::document::*;
use crate::scalar::{double_quoted, needs_escape, plain_safe, resolve_plain, PlainKind};
use crate::{ExtensionValue, OrderedMap, Package};

/// Lines longer than this switch a flow list to block style and fold the package description.
const WIDTH: usize = 100;

enum Value {
    Text(String),
    Raw(String),
    Folded(String),
    List { items: Vec<Value>, block: bool },
    Map(Vec<Entry>),
}

struct Entry {
    key: String,
    value: Value,
    gap: bool,
}

#[derive(Default)]
struct Fields(Vec<Entry>);

impl Fields {
    fn put(&mut self, key: &str, value: Value) {
        self.0.push(Entry {
            key: key.to_string(),
            value,
            gap: false,
        });
    }

    fn text(&mut self, key: &str, value: &str) {
        self.put(key, Value::Text(value.to_string()));
    }

    fn opt(&mut self, key: &str, value: &Option<String>) {
        if let Some(value) = value {
            self.text(key, value);
        }
    }

    fn flag(&mut self, key: &str, value: Option<bool>) {
        if let Some(value) = value {
            self.put(key, Value::Raw(value.to_string()));
        }
    }

    fn list(&mut self, key: &str, values: &[String]) {
        if !values.is_empty() {
            self.put(key, texts(values, false));
        }
    }

    fn maps<T>(&mut self, key: &str, values: &[T], each: fn(&T) -> Fields) {
        if !values.is_empty() {
            self.put(
                key,
                Value::List {
                    items: values
                        .iter()
                        .map(|value| Value::Map(each(value).0))
                        .collect(),
                    block: true,
                },
            );
        }
    }

    fn done(self) -> Value {
        Value::Map(self.0)
    }
}

fn texts(values: &[String], block: bool) -> Value {
    Value::List {
        items: values
            .iter()
            .map(|value| Value::Text(value.clone()))
            .collect(),
        block,
    }
}

fn author(author: &PackageAuthor) -> Fields {
    let mut fields = Fields::default();
    fields.text("name", &author.name);
    fields.opt("email", &author.email);
    fields.opt("url", &author.url);
    if let Some(role) = author.role {
        fields.text(
            "role",
            match role {
                AuthorRole::Author => "author",
                AuthorRole::Maintainer => "maintainer",
                AuthorRole::Contributor => "contributor",
            },
        );
    }
    fields
}

fn secret(secret: &PackageSecret) -> Fields {
    let mut fields = Fields::default();
    fields.text("name", &secret.name);
    fields.text("purpose", &secret.purpose);
    fields.flag("required", secret.required);
    fields
}

fn file(file: &PackageFile) -> Fields {
    let mut fields = Fields::default();
    fields.text("path", &file.path);
    fields.opt("description", &file.description);
    fields.opt("content", &file.content);
    fields
}

fn memory(memory: &PackageMemory) -> Fields {
    let mut fields = Fields::default();
    fields.text("title", &memory.title);
    fields.text(
        "kind",
        match memory.kind {
            MemoryKind::Fact => "fact",
            MemoryKind::Rule => "rule",
            MemoryKind::Learning => "learning",
        },
    );
    fields.text("body", &memory.body);
    fields.list("tags", &memory.tags);
    fields
}

fn skill_file(file: &SkillFile) -> Fields {
    let mut fields = Fields::default();
    fields.text("path", &file.path);
    fields.text("content", &file.content);
    fields
}

fn skill(skill: &PackageSkill) -> Fields {
    let mut fields = Fields::default();
    match skill {
        PackageSkill::Path { path } => fields.text("path", path),
        PackageSkill::Inline(inline) => {
            fields.text("name", &inline.name);
            fields.text("description", &inline.description);
            fields.text("content", &inline.content);
            fields.maps("files", &inline.files, skill_file);
        }
    }
    fields
}

fn automation(automation: &PackageAutomation) -> Fields {
    let mut fields = Fields::default();
    match automation {
        PackageAutomation::Path { path, description } => {
            fields.text("path", path);
            fields.opt("description", description);
        }
        PackageAutomation::Inline {
            name,
            description,
            content,
        } => {
            fields.text("name", name);
            fields.opt("description", description);
            fields.text("content", content);
        }
    }
    fields
}

fn input(input: &PackageInput) -> Value {
    match input {
        PackageInput::Short(description) => Value::Text(description.clone()),
        PackageInput::Full(spec) => {
            let mut fields = Fields::default();
            fields.text("description", &spec.description);
            fields.flag("required", spec.required);
            fields.opt("default", &spec.default);
            fields.list("choices", &spec.choices);
            fields.done()
        }
    }
}

fn role(role: &TeamRole) -> Value {
    let mut fields = Fields::default();
    fields.text("kind", role.kind.as_str());
    fields.opt("name", &role.name);
    fields.opt("description", &role.description);
    match &role.start {
        Some(RoleStart::Mode(StartMode::WithTeam)) => fields.text("start", "with_team"),
        Some(RoleStart::Mode(StartMode::OnDemand)) => fields.text("start", "on_demand"),
        Some(RoleStart::Schedule { schedule }) => {
            let mut window = Fields::default();
            window.text("schedule", schedule);
            fields.put("start", window.done());
        }
        None => {}
    }
    match &role.model {
        Some(ModelPreference::List(models)) => fields.put("model", texts(models, false)),
        Some(ModelPreference::SameAs { same_as }) => {
            let mut reference = Fields::default();
            reference.text("same_as", same_as);
            fields.put("model", reference.done());
        }
        None => {}
    }
    fields.opt("reasoning", &role.reasoning);
    fields.list("skills", &role.skills);
    fields.list("tools", &role.tools);
    fields.list("files", &role.files);
    fields.opt("instructions", &role.instructions);
    fields.opt("instructions_file", &role.instructions_file);
    for (key, value) in role.extensions.iter() {
        fields.put(key, extension(value));
    }
    fields.done()
}

fn roles<'a>(roles: impl Iterator<Item = (&'a str, &'a TeamRole)>) -> Value {
    Value::Map(
        roles
            .enumerate()
            .map(|(index, (name, declared))| Entry {
                key: name.to_string(),
                value: role(declared),
                gap: index > 0,
            })
            .collect(),
    )
}

fn extension(value: &ExtensionValue) -> Value {
    match value {
        ExtensionValue::Scalar { text, plain }
            if *plain && resolve_plain(text) != PlainKind::Str =>
        {
            Value::Raw(text.clone())
        }
        ExtensionValue::Scalar { text, .. } => Value::Text(text.clone()),
        ExtensionValue::Seq(items) => Value::List {
            items: items.iter().map(extension).collect(),
            block: false,
        },
        ExtensionValue::Map(entries) => Value::Map(
            entries
                .iter()
                .map(|(key, value)| Entry {
                    key: key.to_string(),
                    value: extension(value),
                    gap: false,
                })
                .collect(),
        ),
    }
}

/// Top-level blocks; a blank line separates consecutive blocks.
fn top_level(document: &TeamDocument, extensions: &OrderedMap<ExtensionValue>) -> Vec<(u8, Entry)> {
    const IDENTITY: u8 = 0;
    const START: u8 = 1;
    let mut out: Vec<(u8, Entry)> = Vec::new();
    let mut add = |block: u8, key: &str, value: Value| {
        out.push((
            block,
            Entry {
                key: key.to_string(),
                value,
                gap: false,
            },
        ))
    };
    add(IDENTITY, "name", Value::Text(document.name.clone()));
    add(IDENTITY, "version", Value::Text(document.version.clone()));
    if let Some(title) = &document.title {
        add(IDENTITY, "title", Value::Text(title.clone()));
    }
    let folded = format!("description: {}", document.description)
        .chars()
        .count()
        > WIDTH
        && plain_safe(&document.description, false);
    add(
        IDENTITY,
        "description",
        if folded {
            Value::Folded(document.description.clone())
        } else {
            Value::Text(document.description.clone())
        },
    );
    if let Some(icon) = &document.icon {
        add(IDENTITY, "icon", Value::Text(icon.clone()));
    }
    let mut authors = Fields::default();
    authors.maps("authors", &document.authors, author);
    for entry in authors.0 {
        add(IDENTITY, &entry.key, entry.value);
    }
    if let Some(links) = &document.links {
        let mut fields = Fields::default();
        fields.opt("homepage", &links.homepage);
        fields.opt("repository", &links.repository);
        fields.opt("issues", &links.issues);
        fields.opt("docs", &links.docs);
        add(IDENTITY, "links", fields.done());
    }
    if let Some(license) = &document.license {
        add(IDENTITY, "license", Value::Text(license.clone()));
    }
    if !document.when.is_empty() {
        add(IDENTITY, "when", texts(&document.when, true));
    }
    if let Some(lineage) = &document.derived_from {
        let mut fields = Fields::default();
        fields.text("name", &lineage.name);
        fields.text("version", &lineage.version);
        add(IDENTITY, "derived_from", fields.done());
    }
    if let Some(project) = document.project {
        let value = match project {
            ProjectNeed::Required => "required",
            ProjectNeed::NoProject => "none",
        };
        add(START, "project", Value::Text(value.to_string()));
    }
    if let Some(requires) = &document.requires {
        let mut fields = Fields::default();
        fields.list("harnesses", &requires.harnesses);
        add(START, "requires", fields.done());
    }
    if !document.inputs.is_empty() {
        let entries = document
            .inputs
            .iter()
            .map(|(key, value)| Entry {
                key: key.to_string(),
                value: input(value),
                gap: false,
            })
            .collect();
        add(2, "inputs", Value::Map(entries));
    }
    if let Some(permissions) = &document.permissions {
        let mut fields = Fields::default();
        match &permissions.network {
            Some(PackageNetwork::Any(any)) => fields.put("network", Value::Raw(any.to_string())),
            Some(PackageNetwork::Hosts(hosts)) => fields.put("network", texts(hosts, false)),
            None => {}
        }
        fields.flag("repo_write", permissions.repo_write);
        fields.flag("shell", permissions.shell);
        fields.flag("browser", permissions.browser);
        add(3, "permissions", fields.done());
    }
    let mut payload = Fields::default();
    payload.maps("secrets", &document.secrets, secret);
    payload.maps("files", &document.files, file);
    payload.maps("memory", &document.memory, memory);
    payload.maps("skills", &document.skills, skill);
    payload.maps("automation", &document.automation, automation);
    for entry in payload.0 {
        let block = match entry.key.as_str() {
            "secrets" => 4,
            "files" => 5,
            "memory" => 6,
            "skills" => 7,
            _ => 8,
        };
        add(block, &entry.key, entry.value);
    }
    add(9, "roles", roles(document.roles()));
    for (key, value) in extensions.iter() {
        add(10, key, extension(value));
    }
    out
}

struct Writer {
    out: String,
    keep_tail: bool,
}

fn spaces(count: usize) -> String {
    " ".repeat(count)
}

fn key_text(key: &str) -> String {
    if plain_safe(key, false) {
        key.to_string()
    } else {
        double_quoted(key)
    }
}

impl Writer {
    fn line(&mut self, text: &str) {
        self.out.push_str(text);
        self.out.push('\n');
        self.keep_tail = false;
    }

    /// A separator blank line, skipped after a keep-chomped block so its content stays exact.
    fn blank(&mut self) {
        if !self.keep_tail {
            self.out.push('\n');
        }
    }

    /// Writes `head` followed by `text` as a scalar whose block content indents to `indent + 2`.
    fn scalar(&mut self, head: &str, indent: usize, text: &str) {
        let escape = text
            .chars()
            .any(|character| needs_escape(character) || character == '\r');
        let multiline =
            text.contains('\n') && text.chars().any(|character| !character.is_whitespace());
        if !escape && multiline {
            self.literal(head, indent, text);
        } else if !escape && plain_safe(text, false) {
            self.line(&format!("{head} {text}"));
        } else {
            self.line(&format!("{head} {}", double_quoted(text)));
        }
    }

    fn literal(&mut self, head: &str, indent: usize, text: &str) {
        let body = text.trim_end_matches('\n');
        let trailing = text.len() - body.len();
        let indicator = match body.split('\n').find(|line| !line.is_empty()) {
            Some(first) if first.starts_with([' ', '\t']) => "2",
            _ => "",
        };
        let chomp = match trailing {
            0 => "-",
            1 => "",
            _ => "+",
        };
        self.line(&format!("{head} |{indicator}{chomp}"));
        let pad = spaces(indent + 2);
        for line in body.split('\n') {
            if line.is_empty() {
                self.line("");
            } else {
                self.line(&format!("{pad}{line}"));
            }
        }
        for _ in 1..trailing {
            self.line("");
        }
        self.keep_tail = trailing > 1;
    }

    fn flow(items: &[Value]) -> Option<String> {
        let mut rendered = Vec::with_capacity(items.len());
        for item in items {
            match item {
                Value::Raw(raw) => rendered.push(raw.clone()),
                Value::Text(text)
                    if !text.contains('\n')
                        && !text
                            .chars()
                            .any(|character| needs_escape(character) || character == '\r') =>
                {
                    rendered.push(if plain_safe(text, true) {
                        text.clone()
                    } else {
                        double_quoted(text)
                    })
                }
                _ => return None,
            }
        }
        Some(format!("[{}]", rendered.join(", ")))
    }

    fn entry(&mut self, lead: &str, indent: usize, entry: &Entry) {
        let head = format!("{lead}{}:", key_text(&entry.key));
        match &entry.value {
            Value::Text(text) => self.scalar(&head, indent, text),
            Value::Raw(raw) => self.line(&format!("{head} {raw}")),
            Value::Folded(text) => {
                self.line(&format!("{head} >-"));
                self.line(&format!("{}{text}", spaces(indent + 2)));
            }
            Value::List { items, block } => {
                let flow = (!block)
                    .then(|| Self::flow(items))
                    .flatten()
                    .filter(|flow| head.chars().count() + 1 + flow.chars().count() <= WIDTH);
                match flow {
                    Some(flow) => self.line(&format!("{head} {flow}")),
                    None => {
                        self.line(&head);
                        for item in items {
                            self.item(indent + 2, item);
                        }
                    }
                }
            }
            Value::Map(entries) => {
                self.line(&head);
                self.map(entries, indent + 2, None);
            }
        }
    }

    fn item(&mut self, column: usize, value: &Value) {
        let dash = format!("{}-", spaces(column));
        match value {
            Value::Text(text) => self.scalar(&dash, column, text),
            Value::Raw(raw) | Value::Folded(raw) => self.line(&format!("{dash} {raw}")),
            Value::Map(entries) if !entries.is_empty() => {
                self.map(entries, column + 2, Some(column))
            }
            Value::Map(_) => self.line(&format!("{dash} {{}}")),
            Value::List { items, .. } => match Self::flow(items) {
                Some(flow) => self.line(&format!("{dash} {flow}")),
                None => self.line(&format!("{dash} []")),
            },
        }
    }

    fn map(&mut self, entries: &[Entry], indent: usize, dash: Option<usize>) {
        for (index, entry) in entries.iter().enumerate() {
            if entry.gap && index > 0 {
                self.blank();
            }
            let lead = match dash {
                Some(column) if index == 0 => format!("{}- ", spaces(column)),
                _ => spaces(indent),
            };
            self.entry(&lead, indent, entry);
        }
    }
}

/// The one canonical YAML writer for ATP packages.
pub(crate) fn to_yaml(package: &Package) -> String {
    let mut writer = Writer {
        out: String::new(),
        keep_tail: false,
    };
    if let Some(header) = &package.header {
        writer.out.push_str(header);
    }
    let mut previous = None;
    for (block, entry) in top_level(&package.document, &package.extensions) {
        if previous.is_some_and(|previous| previous != block) {
            writer.blank();
        }
        previous = Some(block);
        writer.entry("", 0, &entry);
    }
    writer.out
}
