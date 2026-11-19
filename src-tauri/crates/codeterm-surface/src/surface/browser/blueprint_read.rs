use std::time::{Duration, Instant};

use serde::Serialize;

use super::{
    blueprint_origins::{allows_page_origin, guard_navigation, main_frame, REFUSED_ORIGIN},
    heuristics::NEEDS_NONE,
    verbs::WaitFor,
    BrowserHostConfig, BrowserRuntime, BrowserRuntimeFactory, SurfaceResume, SurfaceRuntime,
};

pub enum PageCapture {
    Text,
    Snapshot,
}

pub struct NamedPageRead<'a> {
    pub profile: &'a str,
    pub url: &'a str,
    pub origins: &'a [String],
    pub wait_text: Option<&'a str>,
    pub limit: usize,
    pub capture: PageCapture,
    pub deadline: Instant,
}

#[derive(Serialize)]
pub struct NamedPageResult {
    outcome: &'static str,
    text: String,
    final_url: String,
    profile: String,
}

impl BrowserRuntimeFactory {
    pub async fn read_named_page(&self, read: NamedPageRead<'_>) -> NamedPageResult {
        let browser = BrowserRuntime::new(
            BrowserHostConfig::under_data_dir(&self.data_dir),
            "Blueprint",
            "blueprint",
            self.swept.clone(),
            SurfaceResume::default(),
        );
        let mut outcome = "navigation_failed";
        let mut final_url = String::new();
        let mut reply = tokio::time::timeout_at(
            read.deadline
                .checked_sub(Duration::from_secs(16))
                .unwrap_or_else(Instant::now)
                .into(),
            async {
                let page = match browser.ensure_page_with_profile(Some(read.profile)).await {
                    Err(error) if error == "browser profile is in use" => {
                        outcome = "browser_busy";
                        return Err(error);
                    }
                    opened => opened?,
                };
                guard_navigation(&page, read.origins, async {
                    browser.goto(&page, read.url).await?;
                    if let Some(text) = read.wait_text {
                        browser.wait(&page, WaitFor::Text(text.to_string())).await?;
                    }
                    let status = browser.shared.state();
                    let before = main_frame(&page).await?;
                    let url = before["url"]
                        .as_str()
                        .filter(|url| allows_page_origin(read.origins, url))
                        .ok_or(REFUSED_ORIGIN)?;
                    final_url = url.to_string();
                    if status.needs_human != NEEDS_NONE {
                        outcome = "needs_human";
                        return Err(format!("the page needs a human: {}", status.needs_human));
                    }
                    let content = match read.capture {
                        PageCapture::Text => {
                            browser.text_verb(&page, &["48000".to_string()]).await?
                        }
                        PageCapture::Snapshot => {
                            browser
                                .snapshot_verb(
                                    &page,
                                    &[
                                        read.limit.to_string(),
                                        "--with-text".to_string(),
                                        "--json".to_string(),
                                    ],
                                )
                                .await?
                        }
                    };
                    let after = main_frame(&page).await?;
                    if before["loaderId"] != after["loaderId"] || before["url"] != after["url"] {
                        return Err("the document changed during capture".into());
                    }
                    outcome = "read";
                    Ok(content.text)
                })
                .await
            },
        )
        .await
        .unwrap_or_else(|_| Err("the step deadline cannot cover the read".into()));
        let status = browser.shared.state();
        if reply.as_ref().is_err_and(|error| error == REFUSED_ORIGIN) {
            outcome = "origin_not_allowed";
        }
        if let Err(error) = browser.close().await {
            outcome = "navigation_failed";
            reply = Err(error.message());
        }
        NamedPageResult {
            outcome,
            text: browser.redact_text(&reply.unwrap_or_else(|error| error)),
            final_url: browser.redact_text(&final_url),
            profile: if status.profile == read.profile {
                status.profile
            } else {
                String::new()
            },
        }
    }
}
