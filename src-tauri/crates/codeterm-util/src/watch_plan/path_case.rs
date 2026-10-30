use std::path::Path;

#[derive(Clone, Copy)]
pub enum PathCase {
    Sensitive,
    Insensitive,
}

impl PathCase {
    pub fn native() -> Self {
        if cfg!(windows) {
            Self::Insensitive
        } else {
            Self::Sensitive
        }
    }

    pub fn key(self, path: &Path) -> String {
        let key = codeterm_paths::strip_verbatim_prefix(&path.to_string_lossy()).replace('\\', "/");
        let key = key.trim_end_matches('/');
        match self {
            Self::Sensitive => key.to_string(),
            Self::Insensitive => key.to_lowercase(),
        }
    }

    pub fn same(self, left: &Path, right: &Path) -> bool {
        if matches!(self, Self::Sensitive) {
            left == right
        } else {
            self.key(left) == self.key(right)
        }
    }

    pub fn within(self, child: &Path, parent: &Path) -> bool {
        if matches!(self, Self::Sensitive) {
            return child.starts_with(parent);
        }
        let child = self.key(child);
        let parent = self.key(parent);
        child == parent
            || child
                .strip_prefix(&parent)
                .is_some_and(|rest| rest.starts_with('/'))
    }

    pub fn direct_child(self, child: &Path, parent: &Path) -> bool {
        if matches!(self, Self::Sensitive) {
            return child.parent() == Some(parent);
        }
        self.key(child)
            .rsplit_once('/')
            .is_some_and(|(directory, _)| directory == self.key(parent))
    }
}
