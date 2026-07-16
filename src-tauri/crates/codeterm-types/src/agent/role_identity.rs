pub enum RoleIdentity<'a> {
    Heartbeat {
        role: &'a str,
        plan_id: &'a str,
        peer_text: &'a str,
    },
    Manager {
        template: &'a str,
        role: &'a str,
        team_id: &'a str,
        team_name: &'a str,
        plan_id: &'a str,
        project: &'a str,
        role_guidance: &'a str,
    },
}

pub fn render(identity: RoleIdentity<'_>) -> String {
    match identity {
        RoleIdentity::Heartbeat { role, plan_id, peer_text } => format!(
            "<codeterm-role-identity plan=\"{plan_id}\">\nYou are the {role} for plan {plan_id}{peer_text}.\n</codeterm-role-identity>"
        ),
        RoleIdentity::Manager { template, role, team_id, team_name, plan_id, project, role_guidance } => {
            render_once(template, &[
                ("{role}", role), ("{team_id}", team_id), ("{team_name}", team_name),
                ("{plan_id}", plan_id), ("{project}", project), ("{role_guidance}", role_guidance),
            ])
        }
    }
}

fn render_once(template: &str, tokens: &[(&str, &str)]) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let Some(end) = rest[start..].find('}') else {
            out.push_str(&rest[start..]);
            return out;
        };
        let end = start + end + 1;
        let token = &rest[start..end];
        out.push_str(
            tokens
                .iter()
                .find_map(|(key, value)| (*key == token).then_some(*value))
                .unwrap_or(token),
        );
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}
