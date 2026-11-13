use crate::diagnostics::{DiagnosticCode as Code, Sink};
use crate::document::*;
use crate::scalar::{resolve_plain, PlainKind};
use crate::yaml::{child_path, Key, Node, NodeValue, ScalarStyle};
use crate::{ExtensionValue, OrderedMap};

const PLAN_TEMPLATE_KEYS: &[&str] = &["plan", "plans", "spec", "specs", "tasks", "goal"];

fn removed_hint(key: &str, top_level: bool) -> Option<&'static str> {
    Some(match key {
        "persona" => "use `name` for a manager's display name",
        "extends" | "includes" => "each role carries its own complete instructions",
        "single" => "a worker that must run alone says so in its instructions",
        "intent" => "authority comes from the host, never from the package",
        "needs" => "placement is host-side registration data",
        "instructions" | "instructions_file" if top_level => {
            "write the team's way of working in the orchestrator's instructions"
        }
        "brief" if top_level => "put a starting hint in the planner's instructions",
        _ => return None,
    })
}

fn indexed(path: &str, index: usize) -> String {
    format!("{path}[{index}]")
}

struct Reader<'s> {
    sink: &'s mut Sink,
}

impl Reader<'_> {
    fn map<'n>(&mut self, node: &'n Node, path: &str) -> Option<&'n [(Key, Node)]> {
        match &node.value {
            NodeValue::Map(entries) => Some(entries),
            _ => {
                self.sink.error(
                    Code::ExpectedMapping,
                    path,
                    Some(node.mark),
                    format!("`{}` must be a mapping", display(path)),
                );
                None
            }
        }
    }

    fn seq<'n>(&mut self, node: &'n Node, path: &str) -> Option<&'n [Node]> {
        match &node.value {
            NodeValue::Seq(items) => Some(items),
            _ => {
                self.sink.error(
                    Code::ExpectedList,
                    path,
                    Some(node.mark),
                    format!("`{}` must be a list", display(path)),
                );
                None
            }
        }
    }

    fn string(&mut self, node: &Node, path: &str) -> Option<String> {
        match &node.value {
            NodeValue::Scalar { text, style }
                if *style != ScalarStyle::Plain || resolve_plain(text) == PlainKind::Str =>
            {
                Some(text.clone())
            }
            _ => {
                self.sink.error(
                    Code::ExpectedString,
                    path,
                    Some(node.mark),
                    format!(
                        "`{}` must be a string (quote numbers, booleans and null)",
                        display(path)
                    ),
                );
                None
            }
        }
    }

    fn boolean(&mut self, node: &Node, path: &str) -> Option<bool> {
        if let NodeValue::Scalar {
            text,
            style: ScalarStyle::Plain,
        } = &node.value
        {
            if let PlainKind::Bool(value) = resolve_plain(text) {
                return Some(value);
            }
        }
        self.sink.error(
            Code::ExpectedBoolean,
            path,
            Some(node.mark),
            format!("`{}` must be true or false", display(path)),
        );
        None
    }

    fn strings(&mut self, node: &Node, path: &str) -> Vec<String> {
        self.list(node, path, |reader, item, path| reader.string(item, path))
    }

    fn list<T>(
        &mut self,
        node: &Node,
        path: &str,
        item: fn(&mut Self, &Node, &str) -> Option<T>,
    ) -> Vec<T> {
        let Some(items) = self.seq(node, path) else {
            return Vec::new();
        };
        items
            .iter()
            .enumerate()
            .filter_map(|(index, value)| item(self, value, &indexed(path, index)))
            .collect()
    }

    fn missing(&mut self, parent: &str, node: &Node, field: &str) {
        let path = child_path(parent, field);
        self.sink.error(
            Code::MissingField,
            &path,
            Some(node.mark),
            format!("`{path}` is required"),
        );
    }

    fn unknown(&mut self, key: &Key, parent: &str) {
        let path = child_path(parent, &key.text);
        let top_level = parent.is_empty();
        if top_level && PLAN_TEMPLATE_KEYS.contains(&key.text.as_str()) {
            self.sink.error(
                Code::PlanTemplateUnsupported,
                &path,
                Some(key.mark),
                format!(
                    "`{}`: Agentic Team Protocol packages carry no plan, spec or task templates",
                    key.text
                ),
            );
        } else if let Some(hint) = removed_hint(&key.text, top_level) {
            self.sink.error(
                Code::RemovedField,
                &path,
                Some(key.mark),
                format!(
                    "`{}` was removed from the Agentic Team Protocol: {hint}",
                    key.text
                ),
            );
        } else {
            self.sink.error(
                Code::UnknownField,
                &path,
                Some(key.mark),
                format!("unknown field `{}`", key.text),
            );
        }
    }

    fn author(&mut self, node: &Node, path: &str) -> Option<PackageAuthor> {
        let entries = self.map(node, path)?;
        let (mut name, mut email, mut url, mut role) = (None, None, None, None);
        for (key, value) in entries {
            let field = child_path(path, &key.text);
            match key.text.as_str() {
                "name" => name = self.string(value, &field),
                "email" => email = self.string(value, &field),
                "url" => url = self.string(value, &field),
                "role" => {
                    role = match self.string(value, &field).as_deref() {
                        Some("author") => Some(AuthorRole::Author),
                        Some("maintainer") => Some(AuthorRole::Maintainer),
                        Some("contributor") => Some(AuthorRole::Contributor),
                        Some(other) => {
                            self.invalid(&field, value, format!("unknown author role `{other}`"));
                            None
                        }
                        None => None,
                    }
                }
                _ => self.unknown(key, path),
            }
        }
        let Some(name) = name else {
            self.missing(path, node, "name");
            return None;
        };
        Some(PackageAuthor {
            name,
            email,
            url,
            role,
        })
    }

    fn invalid(&mut self, path: &str, node: &Node, message: String) {
        self.sink
            .error(Code::InvalidValue, path, Some(node.mark), message);
    }

    fn links(&mut self, node: &Node, path: &str) -> Option<PackageLinks> {
        let entries = self.map(node, path)?;
        let mut links = PackageLinks::default();
        for (key, value) in entries {
            let field = child_path(path, &key.text);
            match key.text.as_str() {
                "homepage" => links.homepage = self.string(value, &field),
                "repository" => links.repository = self.string(value, &field),
                "issues" => links.issues = self.string(value, &field),
                "docs" => links.docs = self.string(value, &field),
                _ => self.unknown(key, path),
            }
        }
        Some(links)
    }

    fn lineage(&mut self, node: &Node, path: &str) -> Option<PackageLineage> {
        let entries = self.map(node, path)?;
        let (mut name, mut version) = (None, None);
        for (key, value) in entries {
            let field = child_path(path, &key.text);
            match key.text.as_str() {
                "name" => name = self.string(value, &field),
                "version" => version = self.string(value, &field),
                _ => self.unknown(key, path),
            }
        }
        match (name, version) {
            (Some(name), Some(version)) => Some(PackageLineage { name, version }),
            (name, _) => {
                let field = if name.is_none() { "name" } else { "version" };
                self.missing(path, node, field);
                None
            }
        }
    }

    fn project(&mut self, node: &Node, path: &str) -> Option<ProjectNeed> {
        match self.string(node, path)?.as_str() {
            "required" => Some(ProjectNeed::Required),
            "none" => Some(ProjectNeed::NoProject),
            other => {
                self.invalid(
                    path,
                    node,
                    format!("project must be `required` or `none`, not `{other}`"),
                );
                None
            }
        }
    }

    fn requires(&mut self, node: &Node, path: &str) -> Option<PackageRequires> {
        let entries = self.map(node, path)?;
        let mut requires = PackageRequires::default();
        for (key, value) in entries {
            match key.text.as_str() {
                "harnesses" => {
                    requires.harnesses = self.strings(value, &child_path(path, "harnesses"))
                }
                _ => self.unknown(key, path),
            }
        }
        Some(requires)
    }

    fn inputs(&mut self, node: &Node, path: &str) -> OrderedMap<PackageInput> {
        let mut inputs = OrderedMap::new();
        let Some(entries) = self.map(node, path) else {
            return inputs;
        };
        for (key, value) in entries {
            let field = child_path(path, &key.text);
            let input = match &value.value {
                NodeValue::Scalar { .. } => self.string(value, &field).map(PackageInput::Short),
                _ => self.input_spec(value, &field).map(PackageInput::Full),
            };
            if let Some(input) = input {
                inputs.insert(key.text.clone(), input);
            }
        }
        inputs
    }

    fn input_spec(&mut self, node: &Node, path: &str) -> Option<PackageInputSpec> {
        let entries = self.map(node, path)?;
        let (mut description, mut required, mut default, mut choices) =
            (None, None, None, Vec::new());
        for (key, value) in entries {
            let field = child_path(path, &key.text);
            match key.text.as_str() {
                "description" => description = self.string(value, &field),
                "required" => required = self.boolean(value, &field),
                "default" => default = self.string(value, &field),
                "choices" => choices = self.strings(value, &field),
                _ => self.unknown(key, path),
            }
        }
        let Some(description) = description else {
            self.missing(path, node, "description");
            return None;
        };
        Some(PackageInputSpec {
            description,
            required,
            default,
            choices,
        })
    }

    fn permissions(&mut self, node: &Node, path: &str) -> Option<PackagePermissions> {
        let entries = self.map(node, path)?;
        let mut permissions = PackagePermissions::default();
        for (key, value) in entries {
            let field = child_path(path, &key.text);
            match key.text.as_str() {
                "network" => {
                    permissions.network = match &value.value {
                        NodeValue::Seq(_) => {
                            Some(PackageNetwork::Hosts(self.strings(value, &field)))
                        }
                        _ => self.boolean(value, &field).map(PackageNetwork::Any),
                    }
                }
                "repo_write" => permissions.repo_write = self.boolean(value, &field),
                "shell" => permissions.shell = self.boolean(value, &field),
                "browser" => permissions.browser = self.boolean(value, &field),
                _ => self.unknown(key, path),
            }
        }
        Some(permissions)
    }

    fn secret(&mut self, node: &Node, path: &str) -> Option<PackageSecret> {
        let entries = self.map(node, path)?;
        let (mut name, mut purpose, mut required) = (None, None, None);
        for (key, value) in entries {
            let field = child_path(path, &key.text);
            match key.text.as_str() {
                "name" => name = self.string(value, &field),
                "purpose" => purpose = self.string(value, &field),
                "required" => required = self.boolean(value, &field),
                _ => self.unknown(key, path),
            }
        }
        match (name, purpose) {
            (Some(name), Some(purpose)) => Some(PackageSecret {
                name,
                purpose,
                required,
            }),
            (name, _) => {
                self.missing(path, node, if name.is_none() { "name" } else { "purpose" });
                None
            }
        }
    }

    fn file(&mut self, node: &Node, path: &str) -> Option<PackageFile> {
        let entries = self.map(node, path)?;
        let (mut file_path, mut description, mut content) = (None, None, None);
        for (key, value) in entries {
            let field = child_path(path, &key.text);
            match key.text.as_str() {
                "path" => file_path = self.string(value, &field),
                "description" => description = self.string(value, &field),
                "content" => content = self.string(value, &field),
                _ => self.unknown(key, path),
            }
        }
        let Some(file_path) = file_path else {
            self.missing(path, node, "path");
            return None;
        };
        Some(PackageFile {
            path: file_path,
            description,
            content,
        })
    }

    fn memory(&mut self, node: &Node, path: &str) -> Option<PackageMemory> {
        let entries = self.map(node, path)?;
        let (mut title, mut kind, mut body, mut tags) = (None, None, None, Vec::new());
        for (key, value) in entries {
            let field = child_path(path, &key.text);
            match key.text.as_str() {
                "title" => title = self.string(value, &field),
                "kind" => {
                    kind = match self.string(value, &field).as_deref() {
                        Some("fact") => Some(MemoryKind::Fact),
                        Some("rule") => Some(MemoryKind::Rule),
                        Some("learning") => Some(MemoryKind::Learning),
                        Some(other) => {
                            self.invalid(
                                &field,
                                value,
                                format!(
                                    "memory kind must be fact, rule or learning, not `{other}`"
                                ),
                            );
                            None
                        }
                        None => None,
                    }
                }
                "body" => body = self.string(value, &field),
                "tags" => tags = self.strings(value, &field),
                _ => self.unknown(key, path),
            }
        }
        match (title, kind, body) {
            (Some(title), Some(kind), Some(body)) => Some(PackageMemory {
                title,
                kind,
                body,
                tags,
            }),
            (title, kind, _) => {
                let field = if title.is_none() {
                    "title"
                } else if kind.is_none() {
                    "kind"
                } else {
                    "body"
                };
                self.missing(path, node, field);
                None
            }
        }
    }

    fn skill(&mut self, node: &Node, path: &str) -> Option<PackageSkill> {
        if let NodeValue::Scalar { .. } = node.value {
            self.sink.error(
                Code::ExpectedMapping,
                path,
                Some(node.mark),
                "a package skill is `{path}` or an inline `{name, description, content, files}`; bare names belong on roles",
            );
            return None;
        }
        let entries = self.map(node, path)?;
        if entries.iter().any(|(key, _)| key.text == "path") {
            let mut skill_path = None;
            for (key, value) in entries {
                match key.text.as_str() {
                    "path" => skill_path = self.string(value, &child_path(path, "path")),
                    _ => self.unknown(key, path),
                }
            }
            return skill_path.map(|path| PackageSkill::Path { path });
        }
        let (mut name, mut description, mut content, mut files) = (None, None, None, Vec::new());
        for (key, value) in entries {
            let field = child_path(path, &key.text);
            match key.text.as_str() {
                "name" => name = self.string(value, &field),
                "description" => description = self.string(value, &field),
                "content" => content = self.string(value, &field),
                "files" => files = self.list(value, &field, Reader::skill_file),
                _ => self.unknown(key, path),
            }
        }
        match (name, description, content) {
            (Some(name), Some(description), Some(content)) => {
                Some(PackageSkill::Inline(InlineSkill {
                    name,
                    description,
                    content,
                    files,
                }))
            }
            (name, description, _) => {
                let field = if name.is_none() {
                    "name"
                } else if description.is_none() {
                    "description"
                } else {
                    "content"
                };
                self.missing(path, node, field);
                None
            }
        }
    }

    fn skill_file(&mut self, node: &Node, path: &str) -> Option<SkillFile> {
        let entries = self.map(node, path)?;
        let (mut file_path, mut content) = (None, None);
        for (key, value) in entries {
            let field = child_path(path, &key.text);
            match key.text.as_str() {
                "path" => file_path = self.string(value, &field),
                "content" => content = self.string(value, &field),
                _ => self.unknown(key, path),
            }
        }
        match (file_path, content) {
            (Some(path), Some(content)) => Some(SkillFile { path, content }),
            (file_path, _) => {
                self.missing(
                    path,
                    node,
                    if file_path.is_none() {
                        "path"
                    } else {
                        "content"
                    },
                );
                None
            }
        }
    }

    fn automation(&mut self, node: &Node, path: &str) -> Option<PackageAutomation> {
        let entries = self.map(node, path)?;
        let (mut item_path, mut name, mut description, mut content) = (None, None, None, None);
        for (key, value) in entries {
            let field = child_path(path, &key.text);
            match key.text.as_str() {
                "path" => item_path = self.string(value, &field),
                "name" => name = self.string(value, &field),
                "description" => description = self.string(value, &field),
                "content" => content = self.string(value, &field),
                _ => self.unknown(key, path),
            }
        }
        if let Some(item_path) = item_path {
            if name.is_some() || content.is_some() {
                self.sink.error(
                    Code::ConflictingFields,
                    path,
                    Some(node.mark),
                    "automation is either `{path, description}` or inline `{name, description, content}`",
                );
                return None;
            }
            return Some(PackageAutomation::Path {
                path: item_path,
                description,
            });
        }
        match (name, content) {
            (Some(name), Some(content)) => Some(PackageAutomation::Inline {
                name,
                description,
                content,
            }),
            (name, _) => {
                self.missing(path, node, if name.is_none() { "name" } else { "content" });
                None
            }
        }
    }

    fn roles(&mut self, node: &Node, path: &str) -> OrderedMap<TeamRole> {
        let mut roles = OrderedMap::new();
        let Some(entries) = self.map(node, path) else {
            return roles;
        };
        for (key, value) in entries {
            if let Some(role) = self.role(value, &child_path(path, &key.text)) {
                roles.insert(key.text.clone(), role);
            }
        }
        if !entries.iter().any(|(key, _)| key.text == ORCHESTRATOR) {
            self.missing(path, node, ORCHESTRATOR);
        }
        roles
    }

    fn kind(&mut self, node: &Node, path: &str) -> Option<RoleKind> {
        let value = self.string(node, path)?;
        let kind = RoleKind::parse(&value);
        if kind.is_none() {
            self.invalid(
                path,
                node,
                format!("kind is `manager` or `worker`, not `{value}`"),
            );
        }
        kind
    }

    fn role(&mut self, node: &Node, path: &str) -> Option<TeamRole> {
        let entries = self.map(node, path)?;
        let mut kind = None;
        let mut role = TeamRole::new(RoleKind::Worker);
        for (key, value) in entries {
            let field = child_path(path, &key.text);
            match key.text.as_str() {
                "kind" => kind = self.kind(value, &field),
                "name" => role.name = self.string(value, &field),
                "description" => role.description = self.string(value, &field),
                "start" => role.start = self.start(value, &field),
                "model" => role.model = self.model(value, &field),
                "reasoning" => role.reasoning = self.string(value, &field),
                "skills" => role.skills = self.strings(value, &field),
                "tools" => role.tools = self.strings(value, &field),
                "files" => role.files = self.strings(value, &field),
                "instructions" => role.instructions = self.string(value, &field),
                "instructions_file" => role.instructions_file = self.string(value, &field),
                extension if extension.starts_with("x-") => {
                    role.extensions
                        .insert(extension.to_string(), extension_value(value));
                }
                _ => self.unknown(key, path),
            }
        }
        if role.instructions.is_some() && role.instructions_file.is_some() {
            self.sink.error(
                Code::ConflictingFields,
                path,
                Some(node.mark),
                "a role has either `instructions` or `instructions_file`, not both",
            );
        }
        if !entries.iter().any(|(key, _)| key.text == "kind") {
            let at = child_path(path, "kind");
            self.sink.error(
                Code::MissingRoleKind,
                &at,
                Some(node.mark),
                format!("`{at}` is required: `manager` or `worker`"),
            );
        }
        role.kind = kind?;
        Some(role)
    }

    fn start(&mut self, node: &Node, path: &str) -> Option<RoleStart> {
        if let NodeValue::Map(entries) = &node.value {
            let mut schedule = None;
            for (key, value) in entries {
                match key.text.as_str() {
                    "schedule" => schedule = self.string(value, &child_path(path, "schedule")),
                    _ => self.unknown(key, path),
                }
            }
            if schedule.is_none() && !entries.iter().any(|(key, _)| key.text == "schedule") {
                self.missing(path, node, "schedule");
            }
            return schedule.map(|schedule| RoleStart::Schedule { schedule });
        }
        match self.string(node, path)?.as_str() {
            "with_team" => Some(RoleStart::Mode(StartMode::WithTeam)),
            "on_demand" => Some(RoleStart::Mode(StartMode::OnDemand)),
            other => {
                self.invalid(
                    path,
                    node,
                    format!("start is `with_team`, `on_demand` or `{{schedule: <min>..<max>}}`, not `{other}`"),
                );
                None
            }
        }
    }

    fn model(&mut self, node: &Node, path: &str) -> Option<ModelPreference> {
        match &node.value {
            NodeValue::Seq(_) => Some(ModelPreference::List(self.strings(node, path))),
            NodeValue::Map(entries) => {
                let mut same_as = None;
                for (key, value) in entries {
                    match key.text.as_str() {
                        "same_as" => same_as = self.string(value, &child_path(path, "same_as")),
                        _ => self.unknown(key, path),
                    }
                }
                if same_as.is_none() && !entries.iter().any(|(key, _)| key.text == "same_as") {
                    self.missing(path, node, "same_as");
                }
                same_as.map(|same_as| ModelPreference::SameAs { same_as })
            }
            NodeValue::Scalar { text, .. } if text == "inherit" => {
                self.sink.error(
                    Code::RemovedInherit,
                    path,
                    Some(node.mark),
                    "`model: inherit` was removed from the Agentic Team Protocol: use `model: {same_as: <role>}`",
                );
                None
            }
            NodeValue::Scalar { .. } => {
                self.sink.error(
                    Code::InvalidModel,
                    path,
                    Some(node.mark),
                    "model is a `harness:model` list or `{same_as: <role>}`",
                );
                None
            }
        }
    }
}

fn display(path: &str) -> &str {
    if path.is_empty() {
        "document"
    } else {
        path
    }
}

pub(crate) fn extension_value(node: &Node) -> ExtensionValue {
    match &node.value {
        NodeValue::Scalar { text, style } => ExtensionValue::Scalar {
            text: text.clone(),
            plain: *style == ScalarStyle::Plain,
        },
        NodeValue::Seq(items) => ExtensionValue::Seq(items.iter().map(extension_value).collect()),
        NodeValue::Map(entries) => {
            let mut map = OrderedMap::new();
            for (key, value) in entries {
                map.insert(key.text.clone(), extension_value(value));
            }
            ExtensionValue::Map(map)
        }
    }
}

/// Reads the typed document; `None` when a required top-level field is missing or malformed.
pub(crate) fn document(
    root: &Node,
    sink: &mut Sink,
) -> Option<(TeamDocument, OrderedMap<ExtensionValue>)> {
    let mut reader = Reader { sink };
    let entries = reader.map(root, "")?;
    let mut extensions = OrderedMap::new();
    let (mut name, mut version, mut title, mut description, mut icon) =
        (None, None, None, None, None);
    let (mut authors, mut links, mut license, mut when, mut derived_from) =
        (Vec::new(), None, None, Vec::new(), None);
    let (mut project, mut requires, mut inputs, mut permissions, mut secrets) =
        (None, None, OrderedMap::new(), None, Vec::new());
    let (mut files, mut memory, mut skills, mut automation) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let mut roles = OrderedMap::new();
    for (key, value) in entries {
        let path = key.text.as_str();
        match path {
            "name" => name = reader.string(value, path),
            "version" => version = reader.string(value, path),
            "title" => title = reader.string(value, path),
            "description" => description = reader.string(value, path),
            "icon" => icon = reader.string(value, path),
            "authors" => authors = reader.list(value, path, Reader::author),
            "links" => links = reader.links(value, path),
            "license" => license = reader.string(value, path),
            "when" => when = reader.strings(value, path),
            "derived_from" => derived_from = reader.lineage(value, path),
            "project" => project = reader.project(value, path),
            "requires" => requires = reader.requires(value, path),
            "inputs" => inputs = reader.inputs(value, path),
            "permissions" => permissions = reader.permissions(value, path),
            "secrets" => secrets = reader.list(value, path, Reader::secret),
            "files" => files = reader.list(value, path, Reader::file),
            "memory" => memory = reader.list(value, path, Reader::memory),
            "skills" => skills = reader.list(value, path, Reader::skill),
            "automation" => automation = reader.list(value, path, Reader::automation),
            "roles" => roles = reader.roles(value, path),
            extension if extension.starts_with("x-") => {
                extensions.insert(extension.to_string(), extension_value(value));
            }
            _ => reader.unknown(key, ""),
        }
    }
    for (field, present) in [
        ("name", entries.iter().any(|(key, _)| key.text == "name")),
        (
            "version",
            entries.iter().any(|(key, _)| key.text == "version"),
        ),
        (
            "description",
            entries.iter().any(|(key, _)| key.text == "description"),
        ),
        ("roles", entries.iter().any(|(key, _)| key.text == "roles")),
    ] {
        if !present {
            reader.missing("", root, field);
        }
    }
    let document = TeamDocument {
        name: name?,
        version: version?,
        title,
        description: description?,
        icon,
        authors,
        links,
        license,
        when,
        derived_from,
        project,
        requires,
        inputs,
        permissions,
        secrets,
        files,
        memory,
        skills,
        automation,
        roles,
    };
    Some((document, extensions))
}
