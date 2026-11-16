use codeterm_neighbors::permissions::Permission;
pub fn grant() -> Permission {
    Permission {
        allowed: true,
        scope: "opaque".into(),
        memory_write_areas: vec!["project".into()],
        ..Default::default()
    }
}
