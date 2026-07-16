#[derive(Debug, Clone, Copy)]
pub struct RoleIdentityFields<'a> {
    pub role: &'a str,
    pub plan_id: &'a str,
    pub peer_role: &'a str,
    pub peer_text: &'a str,
}

pub fn render_role_identity(template: &str, fields: RoleIdentityFields<'_>) -> String {
    render_template(
        template,
        &[
            ("{role}", fields.role),
            ("{plan_id}", fields.plan_id),
            ("{peer_role}", fields.peer_role),
            ("{peer_text}", fields.peer_text),
        ],
    )
}

pub fn render_template(template: &str, replacements: &[(&str, &str)]) -> String {
    let mut rendered = template.to_string();
    for (token, value) in replacements {
        rendered = rendered.replace(token, value);
    }
    rendered
}
