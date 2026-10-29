use crate::{Error, Result, Secret};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use rand::{rngs::OsRng, RngCore};
use sha2::{Digest, Sha256};
use url::Url;

#[derive(Clone, Copy, Default, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LoginHint {
    pub provider: Option<LoginProvider>,
    pub mode: Option<LoginMode>,
}
#[derive(Clone, Copy, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LoginProvider {
    Google,
    Facebook,
    Email,
}
#[derive(Clone, Copy, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LoginMode {
    Signin,
    Signup,
}
impl LoginHint {
    fn append(self, url: &mut Url) {
        if let Some(provider) = self.provider {
            url.query_pairs_mut().append_pair(
                "provider",
                match provider {
                    LoginProvider::Google => "google",
                    LoginProvider::Facebook => "facebook",
                    LoginProvider::Email => "email",
                },
            );
        }
        if let Some(mode) = self.mode {
            url.query_pairs_mut().append_pair(
                "mode",
                match mode {
                    LoginMode::Signin => "signin",
                    LoginMode::Signup => "signup",
                },
            );
        }
    }
}
pub struct LoginFlow {
    hint: LoginHint,
    state: String,
    verifier: Secret,
    redirect: String,
    consumed: bool,
}
pub struct Redemption {
    pub code: Secret,
    pub verifier: Secret,
    pub state: String,
    pub redirect: String,
}
pub struct Callback {
    pub code: String,
    pub state: String,
}
#[derive(Debug, PartialEq, Eq)]
pub enum PasteCallbackError {
    CallbackUrlRequired,
    InvalidCallback,
}
impl std::fmt::Display for PasteCallbackError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::CallbackUrlRequired => "Paste the whole redirected callback URL, including code and state; a bare code is refused",
            Self::InvalidCallback => "Invalid redirected callback URL",
        })
    }
}
impl std::error::Error for PasteCallbackError {}
impl Callback {
    pub fn parse_paste(value: &str) -> std::result::Result<Self, PasteCallbackError> {
        let value = value.trim();
        if !value.contains("://") {
            return Err(PasteCallbackError::CallbackUrlRequired);
        }
        if value.len() > 4096 {
            return Err(PasteCallbackError::InvalidCallback);
        }
        Self::parse(value).map_err(|_| PasteCallbackError::InvalidCallback)
    }
    pub fn parse(value: &str) -> Result<Self> {
        let url = Url::parse(value).map_err(|_| Error::InvalidCallback)?;
        let loopback =
            url.scheme() == "http" && url.host_str() == Some("127.0.0.1") && url.port().is_some();
        let deep =
            url.scheme() == "domios" && url.host_str() == Some("auth") && url.port().is_none();
        if !(loopback || deep)
            || url.path() != "/callback"
            || url.fragment().is_some()
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return Err(Error::InvalidCallback);
        }
        let pairs: Vec<_> = url.query_pairs().collect();
        let one = |key: &str| -> Result<String> {
            let values: Vec<_> = pairs.iter().filter(|(k, _)| k == key).collect();
            if values.len() != 1 {
                return Err(Error::InvalidCallback);
            }
            Ok(values[0].1.to_string())
        };
        let code = one("code")?;
        let state = one("state")?;
        if code.len() != 43
            || !code
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
            || state.len() < 16
            || state.len() > 256
        {
            return Err(Error::InvalidCallback);
        }
        Ok(Self { code, state })
    }
}
impl LoginFlow {
    pub fn new(redirect: String) -> Self {
        let mut state = [0u8; 32];
        let mut verifier = [0u8; 32];
        OsRng.fill_bytes(&mut state);
        OsRng.fill_bytes(&mut verifier);
        Self::from_parts(
            URL_SAFE_NO_PAD.encode(state),
            Secret::new(URL_SAFE_NO_PAD.encode(verifier)),
            redirect,
        )
    }
    pub fn from_parts(state: String, verifier: Secret, redirect: String) -> Self {
        Self {
            hint: LoginHint::default(),
            state,
            verifier,
            redirect,
            consumed: false,
        }
    }
    pub fn with_hint(mut self, hint: LoginHint) -> Self {
        self.hint = hint;
        self
    }
    pub fn challenge(verifier: &str) -> String {
        URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
    }
    pub fn signin_url(&self, base: &str) -> Result<Url> {
        let mut url = Url::parse(base).map_err(|_| Error::Configuration)?;
        if !crate::firebase_endpoints::allows_url(&url)
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(Error::Configuration);
        }
        url.query_pairs_mut()
            .append_pair("state", &self.state)
            .append_pair("code_challenge", &Self::challenge(self.verifier.expose()))
            .append_pair("redirect_uri", &self.redirect);
        self.hint.append(&mut url);
        Ok(url)
    }
    pub fn complete(&mut self, callback: Callback) -> Result<Redemption> {
        if self.consumed {
            return Err(Error::Consumed);
        }
        if callback.code.len() != 43
            || !callback
                .code
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        {
            return Err(Error::InvalidCallback);
        }
        if callback.state != self.state {
            return Err(Error::WrongState);
        }
        self.consumed = true;
        Ok(Redemption {
            code: Secret::new(callback.code),
            verifier: self.verifier.clone(),
            state: self.state.clone(),
            redirect: self.redirect.clone(),
        })
    }
}

#[cfg(test)]
mod tests;
