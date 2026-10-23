use super::*;

struct Fake {
    id: String,
    granted: bool,
}
impl AgentCommandProvider for Fake {
    fn id(&self) -> &str {
        &self.id
    }
    fn granted(&self) -> bool {
        self.granted
    }
    fn on_agent_command(
        &self,
        session_id: &str,
        verb: &str,
        args: &[String],
        _body: Option<&str>,
    ) -> Result<AgentCommandResult, String> {
        Ok(AgentCommandResult {
            result: format!("{session_id}:{verb}:{}", args.join(",")),
            catalogue_changed: false,
        })
    }
}

#[test]
fn registry_resolves_provider() {
    let reg: AgentCommandRegistry = AgentCommandRegistry::new();
    reg.register(
        "local-shell",
        Arc::new(Fake {
            id: "local-shell".into(),
            granted: true,
        }),
    );
    let p = reg.get("local-shell").expect("registered");
    assert_eq!(p.id(), "local-shell");
    assert!(p.granted());
    assert_eq!(
        p.on_agent_command("s1", "say", &["hi".into()], None)
            .unwrap(),
        AgentCommandResult {
            result: "s1:say:hi".into(),
            catalogue_changed: false,
        }
    );
    assert!(reg.get("missing").is_none());
}
