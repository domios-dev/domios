use super::*;

pub(super) struct FixtureProof;

pub(super) fn proof() -> codeterm_account::account_mesh::wire::InstallationProof {
    codeterm_account::account_mesh::wire::InstallationProof {
        schema_version: 1,
        key_id: "1".repeat(64),
        generation: 3,
        timestamp: 1_700_000_000,
        nonce: "fixture-nonce".into(),
        signature: "s".repeat(86),
    }
}

#[async_trait]
impl adapters::cloud::InstallationProofSource for FixtureProof {
    async fn proof(
        &self,
        _: &codeterm_account::Session,
        _: &codeterm_account::InstallationId,
    ) -> Result<codeterm_account::account_mesh::wire::InstallationProof> {
        Ok(proof())
    }
}
