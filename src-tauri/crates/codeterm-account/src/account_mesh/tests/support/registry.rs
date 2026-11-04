use super::*;
use crate::account_mesh::tests::support::wait_for_event;

#[async_trait]
impl MeshRegistry for FakeRegistry {
    async fn revoke_prior_sessions(
        &self,
        _: &Session,
        _: &PriorSessionsTarget,
    ) -> RegistryResult<PriorSessionsRevokedResult> {
        Err(RegistryRefusal::Unavailable)
    }
    async fn challenge(
        &self,
        session: &Session,
        request: &ChallengeRequest,
    ) -> RegistryResult<String> {
        self.admit(session, "challenge");
        let refusal = {
            let mut script = self.script.lock().unwrap();
            script.challenge_once.take().or(script.challenge)
        };
        if let Some(refusal) = refusal {
            return Err(refusal);
        }
        if request.endpoint == ProofEndpoint::MeshAdmission {
            assert!(request.body.get("requested_membership_seconds").is_none());
        }
        Ok(self
            .challenged
            .lock()
            .unwrap()
            .issue(request.body.clone(), self.clock.now()))
    }

    async fn enroll_key(
        &self,
        session: &Session,
        request: &InstallationEnrollment,
    ) -> RegistryResult<KeyEnrollment> {
        let label = match &request.body.operation {
            KeyOperation::Enroll => "enroll_key",
            KeyOperation::Recover => "recover_key",
        };
        self.admit(session, label);
        self.script
            .lock()
            .unwrap()
            .key_requests
            .push(request.clone());
        let gate = self.script.lock().unwrap().enrollment_gate.clone();
        if let Some(gate) = gate {
            self.entered.notify_one();
            wait_for_event(gate.notified()).await;
        }
        if let Some(refusal) = self.script.lock().unwrap().enrollment {
            return Err(refusal);
        }
        let spki = request.body.public_key_spki_base64.clone();
        self.verify(
            session,
            "enrollInstallation",
            &request.body,
            &request.proof,
            Some(&spki),
        )?;
        let mut accounts = self.accounts.lock().unwrap();
        let account = accounts.entry(session.identity.uid.clone()).or_default();
        let assigned = request.body.generation_policy == Some(GenerationPolicy::Assign);
        let generation = if assigned {
            assert_eq!(request.body.operation, KeyOperation::Recover);
            assert_eq!(request.body.expected_generation, 0);
            assert_eq!(request.proof.generation, 0);
            if account.key.as_ref() == Some(&spki) {
                account.key_generation
            } else {
                account.key_generation + 1
            }
        } else {
            let generation = request.body.expected_generation + 1;
            if request.proof.generation != generation {
                return Err(RegistryRefusal::Invalid);
            }
            match &request.body.operation {
                KeyOperation::Enroll
                    if account.key.is_none() && request.body.expected_generation == 0 => {}
                KeyOperation::Recover
                    if account.key.is_some()
                        && account.key_generation == request.body.expected_generation => {}
                _ => return Err(RegistryRefusal::Conflict),
            }
            generation
        };
        account.key = Some(spki);
        account.key_generation = generation;
        if let Some(refusal) = self.script.lock().unwrap().lose_key_reply.take() {
            return Err(refusal);
        }
        Ok(KeyEnrollment {
            generation,
            key_id: request.proof.key_id.clone(),
        })
    }

    async fn admission(
        &self,
        session: &Session,
        envelope: &SignedEnvelope<MeshAdmissionBody>,
    ) -> RegistryResult<MeshResult> {
        let mut result = self.admission_result(session, envelope).await?;
        let mut rows = self.rows(&session.identity.uid, &envelope.installation_proof.key_id);
        if let MeshResult::Membership(membership) = &mut result {
            let payload: serde_json::Value = serde_json::from_slice(
                &URL_SAFE_NO_PAD
                    .decode(&membership.membership.payload)
                    .unwrap(),
            )
            .unwrap();
            if let Some(own) = rows
                .iter_mut()
                .find(|row| row.installation_id == INSTALLATION)
            {
                own.membership_expires_at = payload["expires_at"].as_u64();
                membership.endpoint = own.endpoint.clone();
                membership.presence = own.presence;
                membership.metadata_revision = own.metadata_revision;
                membership.display_name = own.display_name.clone();
            }
        }
        if let MeshResult::Revoked(revoked) = &result {
            if let Some(row) = rows
                .iter_mut()
                .find(|row| row.installation_id == revoked.target_installation_id)
            {
                row.active = false;
                row.eligibility = Eligibility::Revoked;
            }
        }
        if let MeshResult::SessionRevoked(revoked) = &result {
            if let Some(row) = rows
                .iter_mut()
                .find(|row| row.installation_id == revoked.target_installation_id)
            {
                row.presence = RegistryPresence::SignedOut;
            }
        }
        let requested = match &result {
            MeshResult::Membership(value) => value.registry_revision,
            _ => 0,
        };
        let previous = self
            .revision
            .try_update(Ordering::SeqCst, Ordering::SeqCst, |old| {
                Some((old + 1).max(requested))
            })
            .unwrap();
        let revision = (previous + 1).max(requested);
        let revoked: Vec<_> = rows
            .iter()
            .filter(|row| !row.active)
            .map(|row| row.installation_id.clone())
            .collect();
        let grant = self.issuer.sign(&json!({
            "schema_version": 1, "iss": "domios-mesh-registry",
            "aud": {"project": PROJECT, "env": "test"}, "policy_epoch": EPOCH,
            "account_id": account_id(&session.identity.uid), "revision": revision,
            "issued_at": self.clock.now(), "members": rows, "revoked_installations": revoked,
        }));
        match &mut result {
            MeshResult::Membership(value) => {
                if value.signed_registry.is_none() {
                    value.signed_registry = Some(grant.clone());
                }
            }
            MeshResult::Sync(value) => {
                if value.signed_registry.is_none() {
                    value.signed_registry = Some(grant.clone());
                }
            }
            MeshResult::Revoked(value) => {
                if value.signed_registry.is_none() {
                    value.signed_registry = Some(grant.clone());
                }
            }
            MeshResult::SessionRevoked(value) => {
                if value.signed_registry.is_none() {
                    value.signed_registry = Some(grant.clone());
                }
            }
            MeshResult::Recovered(value) => {
                if value.signed_registry.is_none() {
                    value.signed_registry = Some(grant.clone());
                }
            }
            MeshResult::Updated(value) => {
                if value.signed_registry.is_none() {
                    value.signed_registry = Some(grant.clone());
                }
            }
            MeshResult::Published(value) => {
                if value.signed_registry.is_none() {
                    value.signed_registry = Some(grant.clone());
                }
            }
            MeshResult::Withdrawn(value) => {
                if value.signed_registry.is_none() {
                    value.signed_registry = Some(grant.clone());
                }
            }
            MeshResult::Presence(value) => {
                if value.signed_registry.is_none() {
                    value.signed_registry = Some(grant.clone());
                }
            }
            MeshResult::Page(value) => {
                if value.signed_registry.is_none() {
                    value.signed_registry = Some(grant.clone());
                }
            }
        }
        Ok(result)
    }
}

impl FakeRegistry {
    async fn admission_result(
        &self,
        session: &Session,
        envelope: &SignedEnvelope<MeshAdmissionBody>,
    ) -> RegistryResult<MeshResult> {
        let label = match &envelope.request.operation {
            MeshOperation::Enroll { .. } => "enroll",
            MeshOperation::Renew { .. } => "renew",
            MeshOperation::Revoke { .. } => "revoke",
            MeshOperation::SessionRevoke { .. } => "session_revoke",
            MeshOperation::Recover { .. } => "recover",
            MeshOperation::List { .. } => "list",
            MeshOperation::Update { .. } => "update",
            MeshOperation::Publish { .. } => "publish",
            MeshOperation::Sync { .. } => "sync",
            MeshOperation::Withdraw { .. } => "withdraw",
            MeshOperation::Presence { .. } => "presence",
        };
        self.admit(session, label);
        assert_eq!(envelope.request.signed_registry, Some(true));
        if matches!(
            envelope.request.operation,
            MeshOperation::Enroll { .. }
                | MeshOperation::Renew { .. }
                | MeshOperation::Recover { .. }
        ) {
            assert_eq!(envelope.request.own_row, Some(true));
        }
        if matches!(envelope.request.operation, MeshOperation::Recover { .. }) {
            let mut script = self.script.lock().unwrap();
            script.recovery_bodies.push(envelope.request.clone());
            script.recovery_prepared.push(envelope.clone());
        }
        if matches!(envelope.request.operation, MeshOperation::List { .. }) {
            let gate = self.script.lock().unwrap().list_gate.clone();
            if let Some(gate) = gate {
                self.list_entered.notify_one();
                wait_for_event(gate.notified()).await;
            }
        }
        let membership_op = matches!(
            envelope.request.operation,
            MeshOperation::Enroll { .. }
                | MeshOperation::Renew { .. }
                | MeshOperation::Recover { .. }
        );
        if membership_op {
            self.entered.notify_one();
            let gate = self.script.lock().unwrap().gate.clone();
            if let Some(gate) = gate {
                wait_for_event(gate.notified()).await;
            }
        }
        let proof = &envelope.installation_proof;
        self.verify(session, "meshAdmission", &envelope.request, proof, None)?;
        if matches!(envelope.request.operation, MeshOperation::Sync { .. }) {
            let gate = self.script.lock().unwrap().sync_gate.clone();
            if let Some(gate) = gate {
                self.sync_entered.notify_one();
                wait_for_event(gate.notified()).await;
            }
        }
        let script = {
            let mut script = self.script.lock().unwrap();
            let membership = if membership_op {
                script.membership_once.take().or(script.membership)
            } else {
                script.membership
            };
            (membership, script.publication, script.issuer_ahead)
        };
        let lost_membership_reply = if membership_op {
            self.script.lock().unwrap().lose_membership_reply.take()
        } else {
            None
        };
        let uid = session.identity.uid.clone();
        let now = self.clock.now() + script.2;
        let mut accounts = self.accounts.lock().unwrap();
        let account = accounts.entry(uid.clone()).or_default();
        if matches!(
            envelope.request.operation,
            MeshOperation::Recover {
                target_policy: Some(_),
                ..
            }
        ) {
            if let Some((prepared, result)) = &account.recovery_receipt {
                if prepared == envelope {
                    return Ok(result.clone());
                }
            }
        }
        if matches!(envelope.request.operation, MeshOperation::Renew { .. }) {
            let mut script = self.script.lock().unwrap();
            script.renew_bodies.push(envelope.request.clone());
            if !script.disable_renew_replay {
                if let Some((body, result)) = &account.renew_receipt {
                    if body == &envelope.request {
                        return Ok(result.clone());
                    }
                }
            }
        }
        match &envelope.request.operation {
            MeshOperation::Enroll { .. }
            | MeshOperation::Renew { .. }
            | MeshOperation::Recover { .. } => {
                if let Some(refusal) = script.0 {
                    return Err(refusal);
                }
                let slot = match &envelope.request.operation {
                    MeshOperation::Enroll {
                        expected_slot_revision: 0,
                    } if account.slot.is_none()
                        && (!account.inactive || proof.generation > account.revoked_generation) =>
                    {
                        1
                    }
                    MeshOperation::Enroll { .. } if account.inactive => {
                        return Err(RegistryRefusal::Forbidden)
                    }
                    MeshOperation::Enroll { .. } => return Err(RegistryRefusal::Forbidden),
                    MeshOperation::Renew {
                        expected_slot_revision,
                    } if account.slot == Some(*expected_slot_revision)
                        && !account.inactive
                        && account.slot_key.as_ref()
                            == Some(&(proof.key_id.clone(), proof.generation)) =>
                    {
                        expected_slot_revision + 1
                    }
                    MeshOperation::Renew { .. } => return Err(RegistryRefusal::Forbidden),
                    MeshOperation::Recover {
                        target: RecoveryTarget::Own { installation_id },
                        target_policy: Some(OwnTargetPolicy::Own),
                    } if installation_id == INSTALLATION
                        && account.slot.is_some()
                        && account
                            .slot_key
                            .as_ref()
                            .is_some_and(|(_, generation)| proof.generation > *generation) =>
                    {
                        account.slot.unwrap() + 1
                    }
                    MeshOperation::Recover {
                        target: RecoveryTarget::Exact(target),
                        target_policy: None,
                    } if !account.inactive
                        && account.slot == Some(target.slot_revision)
                        && account.slot_key.as_ref()
                            == Some(&(target.key_id.clone(), target.key_generation))
                        && proof.generation > target.key_generation =>
                    {
                        target.slot_revision + 1
                    }
                    MeshOperation::Recover { .. } => return Err(RegistryRefusal::Forbidden),
                    _ => return Err(RegistryRefusal::Invalid),
                };
                account.slot = Some(slot);
                account.slot_key = Some((proof.key_id.clone(), proof.generation));
                account.inactive = false;
                if let Some(name) = self.script.lock().unwrap().rename_race.take() {
                    account.display_name = Some(name);
                    account.metadata_revision += 1;
                }
                // Like the server, a renew never changes presence; only enroll and recover restore it.
                let restores_presence =
                    !matches!(envelope.request.operation, MeshOperation::Renew { .. });
                let mut changed =
                    restores_presence && account.presence != RegistryPresence::Present;
                if restores_presence {
                    account.presence = RegistryPresence::Present;
                }
                if account.display_name.is_none() {
                    if let Some(name) = &envelope.request.default_display_name {
                        account.display_name = Some(name.clone());
                        changed = true;
                    }
                }
                let expires_at = now
                    + self
                        .script
                        .lock()
                        .unwrap()
                        .membership_seconds
                        .unwrap_or(3600);
                let endpoint =
                    envelope
                        .request
                        .endpoint
                        .as_ref()
                        .map(|endpoint| MeshEndpointObservation {
                            transport: endpoint.transport.clone(),
                            tailnet_ip: endpoint.tailnet_ip.clone(),
                            api_port: endpoint.api_port,
                            expires_at,
                        });
                changed |= account.endpoint != endpoint;
                account.endpoint = endpoint;
                if changed {
                    account.metadata_revision += 1;
                }
                let mut payload =
                    Issuer::membership_payload(&uid, &proof.key_id, proof.generation, slot, now);
                if let Some(seconds) = self.script.lock().unwrap().membership_seconds {
                    payload["expires_at"] = json!(now + seconds);
                }
                let signed_registry = self
                    .script
                    .lock()
                    .unwrap()
                    .membership_signed_registry
                    .clone();
                let result = MeshResult::Membership(MembershipResult {
                    endpoint: None,
                    metadata_revision: account.metadata_revision,
                    presence: RegistryPresence::Present,
                    display_name: account.display_name.clone(),
                    signed_registry,
                    operation: match &envelope.request.operation {
                        MeshOperation::Enroll { .. } => MembershipOperation::Enroll,
                        MeshOperation::Renew { .. } => MembershipOperation::Renew,
                        MeshOperation::Recover { .. } => MembershipOperation::Recover,
                        _ => return Err(RegistryRefusal::Invalid),
                    },
                    installation_id: INSTALLATION.into(),
                    slot_revision: slot,
                    registry_revision: self
                        .script
                        .lock()
                        .unwrap()
                        .membership_registry_revision
                        .unwrap_or(slot),
                    membership: self.issuer.sign(&payload),
                });
                if matches!(envelope.request.operation, MeshOperation::Renew { .. }) {
                    account.renew_receipt = Some((envelope.request.clone(), result.clone()));
                }
                if matches!(
                    envelope.request.operation,
                    MeshOperation::Recover {
                        target_policy: Some(_),
                        ..
                    }
                ) {
                    account.recovery_receipt = Some((envelope.clone(), result.clone()));
                }
                if let Some(refusal) = lost_membership_reply {
                    return Err(refusal);
                }
                if let (
                    MeshOperation::Recover {
                        target: RecoveryTarget::Exact(_),
                        ..
                    },
                    MeshResult::Membership(membership),
                ) = (&envelope.request.operation, &result)
                {
                    return Ok(MeshResult::Recovered(RecoveredResult {
                        signed_registry: None,
                        installation_id: membership.installation_id.clone(),
                        key_id: proof.key_id.clone(),
                        key_generation: proof.generation,
                        slot_revision: membership.slot_revision,
                        registry_revision: membership.registry_revision,
                        membership: Some(membership.membership.clone()),
                    }));
                }
                Ok(result)
            }
            MeshOperation::Sync {
                presence,
                endpoint,
                default_display_name,
            } => {
                if let Some(refusal) = script.1 {
                    return Err(refusal);
                }
                if let Some(refusal) = self.script.lock().unwrap().presence {
                    return Err(refusal);
                }
                if account.inactive || account.slot.is_none() {
                    return Err(RegistryRefusal::Forbidden);
                }
                if let Some(name) = self.script.lock().unwrap().rename_race.take() {
                    account.display_name = Some(name);
                    account.metadata_revision += 1;
                }
                let previous = account.metadata_revision;
                let mut changed = account.presence != *presence;
                account.presence = *presence;
                if account.display_name.is_none() {
                    if let Some(name) = default_display_name {
                        account.display_name = Some(name.clone());
                        changed = true;
                    }
                }
                if let Some(endpoint) = endpoint {
                    let observation = MeshEndpointObservation {
                        transport: endpoint.transport.clone(),
                        tailnet_ip: endpoint.tailnet_ip.clone(),
                        api_port: endpoint.api_port,
                        expires_at: now
                            + self.script.lock().unwrap().endpoint_seconds.unwrap_or(3600),
                    };
                    changed |= account.endpoint.as_ref() != Some(&observation);
                    account.endpoint = Some(observation);
                } else {
                    changed |= account.endpoint.is_some();
                    account.endpoint = None;
                }
                if changed {
                    account.metadata_revision += 1;
                }
                let (registry_revision, cursor) = {
                    let script = self.script.lock().unwrap();
                    (
                        script.registry_revision.unwrap_or(9),
                        script.sync_cursor.clone(),
                    )
                };
                let mut result = SyncResult {
                    signed_registry: None,
                    installation_id: INSTALLATION.into(),
                    display_name: account.display_name.clone(),
                    presence: account.presence,
                    publication: if account.endpoint.is_some() {
                        PublicationState::Complete
                    } else {
                        PublicationState::None
                    },
                    endpoint: account.endpoint.clone(),
                    previous_metadata_revision: previous,
                    metadata_revision: account.metadata_revision,
                    slot_revision: account.slot.unwrap(),
                    registry_revision,
                    registry: PageResult {
                        signed_registry: None,
                        registry_revision,
                        rows: Vec::new(),
                        cursor,
                    },
                };
                drop(accounts);
                result.registry.rows = self.rows(&uid, &proof.key_id);
                Ok(MeshResult::Sync(result))
            }
            MeshOperation::Publish {
                expected_metadata_revision,
                endpoint,
            } => {
                if let Some(refusal) = script.1 {
                    return Err(refusal);
                }
                if *expected_metadata_revision != account.metadata_revision {
                    return Err(RegistryRefusal::Conflict);
                }
                let observation = MeshEndpointObservation {
                    transport: endpoint.transport.clone(),
                    tailnet_ip: endpoint.tailnet_ip.clone(),
                    api_port: endpoint.api_port,
                    expires_at: now + self.script.lock().unwrap().endpoint_seconds.unwrap_or(3600),
                };
                account.metadata_revision += 1;
                account.endpoint = Some(observation.clone());
                Ok(MeshResult::Published(PublishedResult {
                    signed_registry: None,
                    installation_id: INSTALLATION.into(),
                    endpoint: observation,
                    previous_metadata_revision: *expected_metadata_revision,
                    metadata_revision: account.metadata_revision,
                    slot_revision: account.slot.unwrap_or(1),
                    registry_revision: self.script.lock().unwrap().registry_revision.unwrap_or(9),
                }))
            }
            MeshOperation::Presence {
                expected_metadata_revision,
                presence,
            } => {
                if let Some(refusal) = self.script.lock().unwrap().presence {
                    return Err(refusal);
                }
                if *expected_metadata_revision != account.metadata_revision {
                    return Err(RegistryRefusal::Forbidden);
                }
                if account.presence != *presence {
                    account.metadata_revision += 1;
                    account.presence = *presence;
                }
                Ok(MeshResult::Presence(PresenceResult {
                    signed_registry: None,
                    installation_id: INSTALLATION.into(),
                    presence: *presence,
                    previous_metadata_revision: *expected_metadata_revision,
                    metadata_revision: account.metadata_revision,
                    slot_revision: account.slot.unwrap_or(1),
                    registry_revision: 10,
                }))
            }
            MeshOperation::Withdraw {
                expected_metadata_revision,
            } => {
                if let Some(refusal) = script.1 {
                    return Err(refusal);
                }
                if *expected_metadata_revision != account.metadata_revision {
                    return Err(RegistryRefusal::Conflict);
                }
                account.metadata_revision += 1;
                account.endpoint = None;
                Ok(MeshResult::Withdrawn(WithdrawnResult {
                    signed_registry: None,
                    installation_id: INSTALLATION.into(),
                    previous_metadata_revision: *expected_metadata_revision,
                    metadata_revision: account.metadata_revision,
                    slot_revision: account.slot.unwrap_or(1),
                    registry_revision: 10,
                }))
            }
            MeshOperation::List { .. } => {
                if account.inactive
                    || account.slot_key.as_ref().is_some_and(|(key, generation)| {
                        key != &proof.key_id || *generation != proof.generation
                    })
                {
                    return Err(RegistryRefusal::Forbidden);
                }
                if let Some(refusal) = self.script.lock().unwrap().list {
                    return Err(refusal);
                }
                drop(accounts);
                let registry_revision = self.script.lock().unwrap().registry_revision.unwrap_or(7);
                let rows = self.rows(&uid, &proof.key_id);
                Ok(MeshResult::Page(PageResult {
                    signed_registry: None,
                    registry_revision,
                    rows,
                    cursor: None,
                }))
            }
            MeshOperation::Update {
                expected_metadata_revision,
                display_name,
            } => {
                if let Some(name) = self.script.lock().unwrap().update_race.take() {
                    account.metadata_revision += 1;
                    account.display_name = Some(name);
                    return Err(RegistryRefusal::Forbidden);
                }
                if self.script.lock().unwrap().reject_update_after_race {
                    account.metadata_revision += 1;
                    return Err(RegistryRefusal::Forbidden);
                }
                if let Some(refusal) = self.script.lock().unwrap().update {
                    return Err(refusal);
                }
                if let Some(name) = self.script.lock().unwrap().rename_race.take() {
                    account.metadata_revision += 1;
                    account.display_name = Some(name);
                }
                if *expected_metadata_revision != account.metadata_revision {
                    return Err(RegistryRefusal::Conflict);
                }
                account.metadata_revision += 1;
                account.display_name = Some(display_name.clone());
                Ok(MeshResult::Updated(UpdatedResult {
                    signed_registry: None,
                    installation_id: INSTALLATION.into(),
                    display_name: display_name.clone(),
                    previous_metadata_revision: *expected_metadata_revision,
                    metadata_revision: account.metadata_revision,
                    slot_revision: account.slot.unwrap_or(1),
                    registry_revision: 8,
                }))
            }
            MeshOperation::SessionRevoke { target } => {
                Ok(MeshResult::SessionRevoked(SessionRevokedResult {
                    signed_registry: None,
                    target_installation_id: target.installation_id.clone(),
                    revoked_at: self.clock.now(),
                }))
            }
            MeshOperation::Revoke { target } => Ok(MeshResult::Revoked(RevokedResult {
                signed_registry: None,
                target_installation_id: target.installation_id.clone(),
                target_key_id: target.key_id.clone(),
                target_key_generation: target.key_generation,
                previous_slot_revision: target.slot_revision,
                slot_revision: target.slot_revision + 1,
                registry_revision: 11,
            })),
        }
    }
}
