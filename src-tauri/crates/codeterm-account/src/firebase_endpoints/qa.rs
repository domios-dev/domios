use super::{Endpoints, PRODUCTION};

pub(super) struct Qa {
    pub project: &'static str,
    pub api_key: &'static str,
    pub app_id: &'static str,
    functions: String,
    signin: String,
    redeem: String,
}

pub(super) fn selected(
    dev_build: bool,
    target: Option<&str>,
    emulator: Option<&str>,
) -> Result<bool, &'static str> {
    if !dev_build {
        return Ok(false);
    }
    match target {
        None | Some("") => Ok(false),
        Some("qa") if emulator == Some("1") => {
            Err("account target: mixed QA and emulator configuration")
        }
        Some("qa") => Ok(true),
        _ => Err("account target: unsupported DOMIOS_ACCOUNT_TARGET (expected qa)"),
    }
}

impl Qa {
    pub fn new(
        project: Option<&'static str>,
        api_key: Option<&'static str>,
        app_id: Option<&'static str>,
        production_key: Option<&str>,
        production_app: Option<&str>,
    ) -> Result<Self, &'static str> {
        let project = required(project, "account target: qa requires DOMIOS_QA_PROJECT_ID")?;
        let api_key = required(
            api_key,
            "account target: qa requires DOMIOS_QA_FIREBASE_API_KEY",
        )?;
        let app_id = required(
            app_id,
            "account target: qa requires DOMIOS_QA_FIREBASE_APP_ID",
        )?;
        if project != "domios-signin-qa"
            || Some(api_key) == production_key.filter(|value| !value.is_empty())
            || Some(app_id) == production_app.filter(|value| !value.is_empty())
        {
            return Err("account target: mixed production/QA Firebase configuration refused");
        }
        let functions = format!("https://us-central1-{project}.cloudfunctions.net/");
        Ok(Self {
            project,
            api_key,
            app_id,
            redeem: format!("{functions}redeemDesktopLoginCode"),
            signin: format!("https://{project}.web.app/signin/"),
            functions,
        })
    }

    pub fn endpoints(&'static self) -> Endpoints {
        Endpoints {
            functions: &self.functions,
            signin: &self.signin,
            redeem: &self.redeem,
            qa: true,
            ..PRODUCTION
        }
    }

    pub fn validate_account(&self, config: &crate::Config) -> Result<(), &'static str> {
        if config.provider != "firebase"
            || config.api_key != self.api_key
            || config.signin_url != self.signin
            || config.redeem_url != self.redeem
        {
            return Err("account target: mixed QA account configuration refused; clear saved account overrides");
        }
        Ok(())
    }
}

fn required(
    value: Option<&'static str>,
    diagnostic: &'static str,
) -> Result<&'static str, &'static str> {
    value
        .filter(|value| !value.trim().is_empty())
        .ok_or(diagnostic)
}

#[cfg(debug_assertions)]
pub(super) fn current() -> Option<&'static Qa> {
    static QA: std::sync::OnceLock<Option<Qa>> = std::sync::OnceLock::new();
    QA.get_or_init(|| {
        let enabled = selected(
            true,
            option_env!("DOMIOS_ACCOUNT_TARGET"),
            std::env::var("DOMIOS_FIREBASE_EMULATOR").ok().as_deref(),
        )
        .unwrap_or_else(|message| panic!("{message}"));
        enabled.then(|| {
            Qa::new(
                option_env!("DOMIOS_QA_PROJECT_ID"),
                option_env!("DOMIOS_QA_FIREBASE_API_KEY"),
                option_env!("DOMIOS_QA_FIREBASE_APP_ID"),
                option_env!("DOMIOS_FIREBASE_API_KEY"),
                option_env!("DOMIOS_FIREBASE_APP_ID"),
            )
            .unwrap_or_else(|message| panic!("{message}"))
        })
    })
    .as_ref()
}
