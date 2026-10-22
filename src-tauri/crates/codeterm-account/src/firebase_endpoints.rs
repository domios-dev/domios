#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Endpoints {
    pub identity_toolkit: &'static str,
    pub secure_token: &'static str,
    pub functions: &'static str,
    pub signin: &'static str,
    pub redeem: &'static str,
    pub emulator: bool,
}

pub const PRODUCTION: Endpoints = Endpoints {
    identity_toolkit: "https://identitytoolkit.googleapis.com/v1/",
    secure_token: "https://securetoken.googleapis.com/v1/",
    functions: "https://us-central1-domios-d9357.cloudfunctions.net/",
    signin: "https://auth.domios.dev/signin/",
    redeem: "https://us-central1-domios-d9357.cloudfunctions.net/redeemDesktopLoginCode",
    emulator: false,
};

#[cfg(any(debug_assertions, test))]
const EMULATOR: Endpoints = Endpoints {
    identity_toolkit: "http://127.0.0.1:19099/identitytoolkit.googleapis.com/v1/",
    secure_token: "http://127.0.0.1:19099/securetoken.googleapis.com/v1/",
    functions: "http://127.0.0.1:15001/demo-domios/us-central1/",
    signin: "http://127.0.0.1:15000/signin/",
    redeem: "http://127.0.0.1:15001/demo-domios/us-central1/redeemDesktopLoginCode",
    emulator: true,
};

pub fn current() -> Endpoints {
    #[cfg(debug_assertions)]
    {
        resolve(
            true,
            std::env::var("DOMIOS_FIREBASE_EMULATOR").ok().as_deref(),
        )
    }
    #[cfg(not(debug_assertions))]
    {
        PRODUCTION
    }
}

#[cfg(any(debug_assertions, test))]
fn resolve(dev_build: bool, opt_in: Option<&str>) -> Endpoints {
    if dev_build && opt_in == Some("1") {
        EMULATOR
    } else {
        PRODUCTION
    }
}

pub fn allows_url(url: &url::Url) -> bool {
    url.username().is_empty()
        && url.password().is_none()
        && (url.scheme() == "https" || allows_emulator_url(url))
}

fn allows_emulator_url(url: &url::Url) -> bool {
    #[cfg(debug_assertions)]
    {
        matches_emulator_url(url, current())
    }
    #[cfg(not(debug_assertions))]
    {
        let _ = url;
        false
    }
}

#[cfg(any(debug_assertions, test))]
fn matches_emulator_url(url: &url::Url, endpoints: Endpoints) -> bool {
    endpoints.emulator
        && url.username().is_empty()
        && url.password().is_none()
        && [
            endpoints.identity_toolkit,
            endpoints.secure_token,
            endpoints.functions,
            endpoints.signin,
        ]
        .iter()
        .any(|base| url.as_str().starts_with(base))
}

pub(crate) fn account_config(mut config: crate::Config) -> crate::Config {
    let endpoints = current();
    apply_account_endpoints(&mut config, cfg!(debug_assertions), endpoints);
    config
}

fn apply_account_endpoints(config: &mut crate::Config, dev_build: bool, endpoints: Endpoints) {
    let endpoints = if dev_build { endpoints } else { PRODUCTION };
    if !dev_build || endpoints.emulator {
        config.signin_url = endpoints.signin.into();
        config.redeem_url = endpoints.redeem.into();
    }
    if endpoints.emulator {
        config.provider = "firebase".into();
        config.api_key = "FAKE-local-api-key".into();
    }
}

#[cfg(test)]
mod tests;
