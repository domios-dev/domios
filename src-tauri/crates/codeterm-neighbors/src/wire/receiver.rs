use super::*;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct EffectiveReceiver {
    pub installation_id: Option<String>,
    pub enc_key_id: Option<String>,
    pub machine_label: Option<String>,
    pub revision: u64,
    pub receiving: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PeerReceiver {
    pub enc_key_id: Option<String>,
    pub revision: u64,
    pub receiving: bool,
}

impl PeerKeys {
    pub fn require_sender_receiver(&self, installation: &str) -> Result<()> {
        match &self.my_receiver {
            Some(receiver) if receiver.revision > MAX_SAFE_INTEGER => Err(Error::InvalidEnvelope),
            Some(receiver)
                if !receiver.receiving
                    || receiver.installation_id.as_deref() != Some(installation) =>
            {
                Err(Error::NotReceiver)
            }
            Some(receiver) if receiver.enc_key_id.is_none() => Err(Error::InvalidEnvelope),
            Some(_) => Ok(()),
            None => Err(Error::NotReceiver),
        }
    }

    pub fn receiver_key(&self) -> Result<&PeerKey> {
        match &self.peer_receiver {
            Some(receiver) if receiver.receiving && receiver.revision <= MAX_SAFE_INTEGER => self
                .keys
                .iter()
                .find(|key| Some(key.enc_key_id.as_str()) == receiver.enc_key_id.as_deref())
                .ok_or(Error::RecipientNotReceiving),
            Some(_) => Err(Error::RecipientNotReceiving),
            None => Err(Error::RecipientNotReceiving),
        }
    }
}

#[cfg(test)]
mod tests;
