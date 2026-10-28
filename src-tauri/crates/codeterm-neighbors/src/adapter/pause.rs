use super::*;

impl FirebaseTransport {
    pub async fn set_paused(&self, friendship: &str, paused: bool) -> Result<()> {
        if !crate::wire::hex_id(friendship, 64) {
            return Err(Error::InvalidRequest);
        }
        let result = self.calls.invoke("friendsSetPaused", json!({
            "installation_id":self.installation_id,"friendship_id":friendship,"paused":paused
        })).await?;
        if result["schema_version"] != 1
            || result["friendship_id"] != friendship
            || result["paused"] != paused
        {
            return Err(Error::Transport);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
