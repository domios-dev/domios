use super::{async_trait, io, note, AccountMeshRecord, AccountMeshStore, Arc, Log, Mutex, Notify};

pub struct MemoryStore {
    pub record: Mutex<AccountMeshRecord>,
    pub corrupt: Mutex<bool>,
    pub fail_save: Mutex<bool>,
    pub save_gate: Mutex<Option<Arc<Notify>>>,
    pub save_entered: Notify,
    pub(super) log: Log,
}
#[async_trait]
impl AccountMeshStore for MemoryStore {
    async fn load(&self) -> io::Result<AccountMeshRecord> {
        if *self.corrupt.lock().unwrap() {
            return Err(io::ErrorKind::InvalidData.into());
        }
        Ok(self.record.lock().unwrap().clone())
    }
    async fn save(&self, record: &AccountMeshRecord) -> io::Result<()> {
        if *self.fail_save.lock().unwrap() {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        let gate = self.save_gate.lock().unwrap().take();
        if let Some(gate) = gate {
            self.save_entered.notify_one();
            gate.notified().await;
        }
        note(&self.log, format!("save:signed_out={}", record.signed_out));
        *self.record.lock().unwrap() = record.clone();
        Ok(())
    }
}
