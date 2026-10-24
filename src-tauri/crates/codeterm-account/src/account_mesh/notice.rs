use super::{AccountMesh, AccountMeshRefusal};
use std::collections::HashMap;

#[derive(Default)]
pub(super) struct RegistryNotices {
    epoch: u64,
    senders: HashMap<String, Sender>,
    running: bool,
    pending: bool,
}

struct Sender {
    revision: u64,
    window: u64,
    count: u8,
}

#[derive(PartialEq, Eq, Debug)]
enum Claim {
    Ignore,
    Queued,
    Run,
}

impl RegistryNotices {
    fn claim(&mut self, epoch: u64, sender: &str, revision: u64, now: u64) -> Claim {
        if self.epoch != epoch {
            *self = Self {
                epoch,
                ..Self::default()
            };
        }
        match self.senders.get_mut(sender) {
            Some(previous) => {
                if revision <= previous.revision {
                    return Claim::Ignore;
                }
                if now.saturating_sub(previous.window) >= 10 {
                    previous.window = now;
                    previous.count = 0;
                }
                if previous.count >= 2 {
                    return Claim::Ignore;
                }
                previous.revision = revision;
                previous.count += 1;
            }
            None => {
                if self.senders.len() >= 256 {
                    return Claim::Ignore;
                }
                self.senders.insert(
                    sender.to_owned(),
                    Sender {
                        revision,
                        window: now,
                        count: 1,
                    },
                );
            }
        }
        if self.running {
            self.pending = true;
            Claim::Queued
        } else {
            self.running = true;
            Claim::Run
        }
    }

    fn next(&mut self, epoch: u64) -> bool {
        if self.epoch != epoch {
            return false;
        }
        if std::mem::take(&mut self.pending) {
            true
        } else {
            self.running = false;
            false
        }
    }

    fn stop(&mut self, epoch: u64) {
        if self.epoch == epoch {
            self.running = false;
            self.pending = false;
        }
    }
}

struct NoticeFlight<'a> {
    mesh: &'a AccountMesh,
    epoch: u64,
    active: bool,
}

impl Drop for NoticeFlight<'_> {
    fn drop(&mut self) {
        if self.active {
            self.mesh
                .with(|inner| inner.registry_notices.stop(self.epoch));
        }
    }
}

impl AccountMesh {
    pub async fn presence_notice(
        &self,
        sender: &str,
        revision: u64,
    ) -> Result<bool, AccountMeshRefusal> {
        let epoch = self.with(|inner| {
            inner
                .state
                .signed_in_account()
                .ok_or(AccountMeshRefusal::SignedOut)?;
            Ok::<_, AccountMeshRefusal>(inner.state.account_epoch)
        })?;
        let claim = self.with(|inner| {
            inner
                .registry_notices
                .claim(epoch, sender, revision, self.now())
        });
        if claim != Claim::Run {
            return Ok(false);
        }
        let mut flight = NoticeFlight {
            mesh: self,
            epoch,
            active: true,
        };
        loop {
            self.list().await?;
            self.push().await;
            if !self.with(|inner| inner.registry_notices.next(epoch)) {
                flight.active = false;
                return Ok(true);
            }
        }
    }
}

#[cfg(test)]
mod tests;
