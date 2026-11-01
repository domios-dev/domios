use super::BrowserRuntime;
use codeterm_browser::profile::instance::{health, verify_dev_identity};

#[derive(Debug, PartialEq, Eq)]
pub(super) struct Target {
    pub name: String,
    pub port: u16,
    pub ephemeral: bool,
}
pub(super) fn parse(args: &[String]) -> Result<Target, String> {
    let mut name = None;
    let mut port = None;
    let mut ephemeral = false;
    let mut i = 0;
    while i < args.len() {
        let flag = args[i].as_str();
        if flag == "--ephemeral" {
            ephemeral = true;
            i += 1;
            continue;
        }
        i += 1;
        let value = args.get(i).ok_or("invalid_instance_options")?;
        match flag {
            "--domios-instance" if name.is_none() => name = Some(value.clone()),
            "--api-port" if port.is_none() => port = Some(value.parse::<u16>().map_err(|_| "invalid_api_port")?),
            _ => return Err("invalid_instance_options: use --domios-instance NAME --api-port PORT [--ephemeral]".into()),
        }
        i += 1;
    }
    let target = Target {
        name: name.ok_or("DEV instance name required")?,
        port: port.ok_or("explicit API port required")?,
        ephemeral,
    };
    if target.port == 0 || target.port == codeterm_paths::PRODUCTION_PORT {
        return Err("refused_production_port".into());
    }
    codeterm_paths::explicit_development_data_dir(&target.name)?;
    Ok(target)
}

fn read_token(root: &std::path::Path) -> Result<String, String> {
    let metadata = std::fs::symlink_metadata(root).map_err(|_| "dev_instance_not_installed")?;
    if !metadata.is_dir() || codeterm_util::safe_tree::is_link(&metadata) {
        return Err("invalid_dev_instance_directory".into());
    }
    let root = std::fs::canonicalize(root).map_err(|_| "dev_instance_not_installed")?;
    let secret = root.join(".secrets");
    codeterm_util::safe_tree::refuse_linked_ancestors(&secret)?;
    if !secret.is_file() || !codeterm_util::private_file::owner_only(&secret).unwrap_or(false) {
        return Err("protected_dev_credential_unavailable".into());
    }
    codeterm_util::instance_token::read_at(&root)?
        .ok_or_else(|| "DEV instance has no stored API credential".into())
}

impl BrowserRuntime {
    pub(super) async fn open_domios_instance(&self, args: &[String]) -> Result<String, String> {
        let target = parse(args)?;
        let origin = format!("http://127.0.0.1:{}", target.port);
        verify_dev_identity(&health(&origin).await?, &target.name, target.port)?;
        let root = codeterm_paths::explicit_development_data_dir(&target.name)?;
        let token = tokio::task::spawn_blocking(move || read_token(&root))
            .await
            .map_err(|_| "dev_auth_task_failed")??;
        self.remember_secret(&token)?;
        let profile = format!(
            "dev-{}-{}",
            target.port,
            target.name.chars().take(24).collect::<String>()
        );
        let mut session = vec![
            "--profile".into(),
            profile,
            "--origin".into(),
            origin.clone(),
        ];
        if target.ephemeral {
            session.push("--ephemeral".into());
        }
        self.open_session(&session).await?;
        verify_dev_identity(&health(&origin).await?, &target.name, target.port)?;
        let page = self.ensure_page().await?;
        self.connect_token(&page, &origin, &token).await
    }
}

#[cfg(test)]
mod tests;
