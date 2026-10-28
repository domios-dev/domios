use super::BrowserRuntime;
use codeterm_browser::profile::{
    delete_named_profile,
    instance::{check_build, normalize_origin},
};
use serde_json::json;

#[derive(Debug, PartialEq, Eq)]
pub(super) struct SessionArgs {
    pub profile: String,
    pub origin: String,
    pub build: Option<String>,
    pub ephemeral: bool,
}

pub(super) fn parse(args: &[String]) -> Result<SessionArgs, String> {
    let mut profile = None;
    let mut origin = None;
    let mut build = None;
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
        let value = args
            .get(i)
            .ok_or("invalid_session: missing option value")?
            .clone();
        let slot = match flag {
            "--profile" => &mut profile,
            "--origin" => &mut origin,
            "--expect-build" => &mut build,
            _ => return Err("invalid_session: unknown option".into()),
        };
        if slot.replace(value).is_some() {
            return Err("invalid_session: duplicate option".into());
        }
        i += 1;
    }
    Ok(SessionArgs {
        profile: profile.ok_or("invalid_session: --profile required")?,
        origin: normalize_origin(&origin.ok_or("invalid_session: --origin required")?)?,
        build,
        ephemeral,
    })
}

impl BrowserRuntime {
    pub(super) async fn open_session(&self, args: &[String]) -> Result<String, String> {
        let args = parse(args)?;
        if let Some(build) = &args.build {
            check_build(&args.origin, build).await?;
        }
        let layout = self.layout.clone();
        let profile = args.profile.clone();
        let origin = args.origin.clone();
        tokio::task::spawn_blocking(move || {
            layout.prepare_instance(&profile, &origin, args.ephemeral)
        })
        .await
        .map_err(|_| "profile_task_failed")??;
        self.cancel_resume().await;
        let viewport = self.host.viewport();
        let opened = self
            .host
            .open(codeterm_browser::OpenOptions {
                profile: args.profile.clone(),
                width: viewport.width as f64,
                height: viewport.height as f64,
                dpr: viewport.dpr,
            })
            .await?;
        *self.profile_request.lock() = args.profile.clone();
        if !opened.reused {
            self.shared.unbind();
        }
        let page = self.host.page().ok_or("browser has no page")?;
        self.shared.bind(&page);
        self.shared
            .update(|state| state.profile = opened.profile.clone());
        self.shared
            .remember_profile(if args.ephemeral { "" } else { &opened.profile });
        self.refresh_profiles().await;
        let page = self.ensure_page().await?;
        self.goto(&page, &format!("{}/web/", args.origin)).await?;
        Ok("opened origin-bound session; snapshot to inspect readiness".into())
    }

    pub(super) async fn connect_session(&self, args: &[String]) -> Result<String, String> {
        let (token, options) = args.split_last().ok_or("connect requires stdin token")?;
        let options = parse(options)?;
        if let Some(build) = &options.build {
            check_build(&options.origin, build).await?;
        }
        if *self.profile_request.lock() != options.profile {
            return Err("profile_mismatch: reopen the requested session".into());
        }
        let page = self.ensure_page().await?;
        self.connect_token(&page, &options.origin, token).await
    }

    pub(super) async fn connect_token(
        &self,
        page: &codeterm_browser::PageChannel,
        expected_origin: &str,
        token: &str,
    ) -> Result<String, String> {
        let origin = self.evaluate(page, "location.origin", false).await?;
        if origin.as_str() != Some(expected_origin) {
            return Err(
                "origin_unconfirmed: wait for the requested origin before connecting".into(),
            );
        }
        self.send(page, "DOMStorage.enable", json!({})).await?;
        self.send(page,"DOMStorage.setDOMStorageItem",json!({"storageId":{"securityOrigin":expected_origin,"isLocalStorage":true},"key":format!("codeterm-web-token:{}",expected_origin),"value":token})).await?;
        let stored = self
            .send(
                page,
                "DOMStorage.getDOMStorageItems",
                json!({"storageId":{"securityOrigin":expected_origin,"isLocalStorage":true}}),
            )
            .await?;
        let key = format!("codeterm-web-token:{}", expected_origin);
        if !stored
            .get("entries")
            .and_then(|v| v.as_array())
            .is_some_and(|entries| {
                entries.iter().any(|entry| {
                    entry.get(0).and_then(|v| v.as_str()) == Some(key.as_str())
                        && entry.get(1).and_then(|v| v.as_str()) == Some(token)
                })
            })
        {
            return Err("token_storage_unconfirmed: inspect before continuing".into());
        }
        self.navigate(page, super::input::NavAction::Reload, None)
            .await?;
        Ok(
            "connection token stored for the requested origin; snapshot to verify authentication"
                .into(),
        )
    }

    pub(super) async fn profile_verb(&self, args: &[String]) -> Result<String, String> {
        match args.first().map(String::as_str) {
            Some("list") if args.len() == 1 => {
                self.refresh_profiles().await;
                Ok(self.profile_view().to_string())
            }
            Some("remove") if args.len() == 2 => {
                delete_named_profile(&self.layout, &args[1]).await?;
                self.refresh_profiles().await;
                Ok("profile removed".into())
            }
            _ => Err("usage: profile list | profile remove <name>".into()),
        }
    }
}

#[cfg(test)]
mod tests;
