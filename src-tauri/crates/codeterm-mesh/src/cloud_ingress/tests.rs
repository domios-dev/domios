use super::authentication::BootstrapResponder;

#[test]
fn bootstrap_requires_the_creation_secret_and_a_fresh_session_binding() {
    fn offer(psk: &[u8; 32]) -> (snow::HandshakeState, Vec<u8>) {
        let mut state = snow::Builder::new("Noise_NNpsk0_25519_ChaChaPoly_SHA256".parse().unwrap())
            .prologue(b"domios-cloud-ingress-bootstrap-v1")
            .unwrap()
            .psk(0, psk)
            .unwrap()
            .build_initiator()
            .unwrap();
        let mut bytes = vec![0; 48];
        assert_eq!(state.write_message(&[], &mut bytes).unwrap(), 48);
        (state, bytes)
    }
    let psk = [3; 32];
    let (mut home, first) = offer(&psk);
    assert!(BootstrapResponder::new(&[4; 32])
        .unwrap()
        .respond(&first)
        .is_err());
    let mut cloud = BootstrapResponder::new(&psk)
        .unwrap()
        .respond(&first)
        .unwrap();
    home.read_message(&cloud.reply, &mut []).unwrap();
    let mut first_channel = home.into_transport_mode().unwrap();
    let mut ciphertext = [0; 19];
    first_channel
        .write_message(b"one", &mut ciphertext)
        .unwrap();
    let mut result = [0; 3];
    assert_eq!(
        cloud
            .transport
            .read_message(&ciphertext, &mut result)
            .unwrap(),
        3
    );
    assert_eq!(&result, b"one");
    assert!(cloud
        .transport
        .read_message(&ciphertext, &mut result)
        .is_err());
    let (_, second) = offer(&psk);
    let mut other = BootstrapResponder::new(&psk)
        .unwrap()
        .respond(&second)
        .unwrap();
    assert!(other
        .transport
        .read_message(&ciphertext, &mut result)
        .is_err());
}

#[test]
fn direct_ingress_requires_public_ipv4_on_the_dedicated_port() {
    use std::net::SocketAddrV4;

    for (octets, port) in [
        ([127, 0, 0, 1], 7687),
        ([10, 0, 0, 1], 7687),
        ([100, 64, 1, 2], 7687),
        ([8, 8, 8, 8], 443),
    ] {
        assert!(!super::direct_address_allowed(SocketAddrV4::new(
            octets.into(),
            port,
        )));
    }
    assert!(super::direct_address_allowed(
        "8.8.8.8:7687".parse().unwrap()
    ));
}

#[tokio::test]
async fn provider_proxy_host_validation_and_dial_refusal() {
    for valid in [
        "domios-a1b2c3d4.fly.dev",
        "a.co",
        "cloud-1.eu-west.example.test",
    ] {
        assert!(
            super::provider_proxy_host_allowed(valid),
            "expected {valid}"
        );
    }
    let overlong = format!("{}.fly.dev", "a".repeat(63).repeat(4));
    for invalid in [
        "",
        "Domios.fly.dev",
        "198.51.100.10",
        "8.8.8.8",
        "::1",
        "[::1]",
        "domios.fly.dev:443",
        "domios.fly.dev:7687",
        "wss://domios.fly.dev",
        "domios.fly.dev/path",
        "domios.fly.dev.",
        ".domios.fly.dev",
        "-bad.fly.dev",
        "bad-.fly.dev",
        "bad..fly.dev",
        "localhost",
        "a b.fly.dev",
        overlong.as_str(),
    ] {
        assert!(
            !super::provider_proxy_host_allowed(invalid),
            "expected rejection for {invalid}"
        );
        let route = super::CloudRoute::ProviderProxy {
            host: invalid.to_owned(),
        };
        assert_eq!(
            super::dial::open(&route, "/cloud/enrolled/v1").await.err(),
            Some(super::IngressError::Rejected)
        );
    }
}

mod test_pki {
    use ed25519_dalek::{Signer, SigningKey};
    use rustls_pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};

    fn der_tlv(tag: u8, body: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(body.len() + 4);
        out.push(tag);
        if body.len() < 128 {
            out.push(body.len() as u8);
        } else if body.len() < 256 {
            out.push(0x81);
            out.push(body.len() as u8);
        } else {
            out.push(0x82);
            out.push((body.len() >> 8) as u8);
            out.push((body.len() & 0xff) as u8);
        }
        out.extend_from_slice(body);
        out
    }

    fn der_seq(parts: &[&[u8]]) -> Vec<u8> {
        let mut body = Vec::new();
        for part in parts {
            body.extend_from_slice(part);
        }
        der_tlv(0x30, &body)
    }

    fn der_set(parts: &[&[u8]]) -> Vec<u8> {
        let mut body = Vec::new();
        for part in parts {
            body.extend_from_slice(part);
        }
        der_tlv(0x31, &body)
    }

    fn der_oid(encoded: &[u8]) -> Vec<u8> {
        der_tlv(0x06, encoded)
    }

    fn der_bit_string(unused: u8, bytes: &[u8]) -> Vec<u8> {
        let mut body = Vec::with_capacity(bytes.len() + 1);
        body.push(unused);
        body.extend_from_slice(bytes);
        der_tlv(0x03, &body)
    }

    fn ed25519_alg_id() -> Vec<u8> {
        der_seq(&[&der_oid(&[0x2b, 0x65, 0x70])])
    }

    fn ed25519_spki(key: &SigningKey) -> Vec<u8> {
        der_seq(&[
            &ed25519_alg_id(),
            &der_bit_string(0, key.verifying_key().as_bytes()),
        ])
    }

    fn ed25519_pkcs8(key: &SigningKey) -> PrivateKeyDer<'static> {
        let mut der = vec![
            0x30, 0x2e, 0x02, 0x01, 0x00, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x04, 0x22,
            0x04, 0x20,
        ];
        der.extend_from_slice(&key.to_bytes());
        PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(der))
    }

    fn cn_name(cn: &str) -> Vec<u8> {
        let atv = der_seq(&[&der_oid(&[0x55, 0x04, 0x03]), &der_tlv(0x0c, cn.as_bytes())]);
        der_seq(&[&der_set(&[&atv])])
    }

    fn extension(oid: &[u8], critical: bool, value: &[u8]) -> Vec<u8> {
        let oid_der = der_oid(oid);
        let val_der = der_tlv(0x04, value);
        if critical {
            let crit = der_tlv(0x01, &[0xff]);
            der_seq(&[&oid_der, &crit, &val_der])
        } else {
            der_seq(&[&oid_der, &val_der])
        }
    }

    fn sign_x509_v3(
        serial: u8,
        issuer_cn: &str,
        subject_cn: &str,
        subject_key: &SigningKey,
        issuer_key: &SigningKey,
        extensions: &[Vec<u8>],
    ) -> CertificateDer<'static> {
        let version = der_tlv(0xa0, &der_tlv(0x02, &[2]));
        let serial_der = der_tlv(0x02, &[serial]);
        let alg = ed25519_alg_id();
        let issuer = cn_name(issuer_cn);
        let validity = der_seq(&[
            &der_tlv(0x17, b"200101000000Z"),
            &der_tlv(0x17, b"491231235959Z"),
        ]);
        let subject = cn_name(subject_cn);
        let spki = ed25519_spki(subject_key);
        let ext_refs: Vec<&[u8]> = extensions.iter().map(Vec::as_slice).collect();
        let exts = der_tlv(0xa3, &der_seq(&ext_refs));
        let tbs = der_seq(&[
            &version,
            &serial_der,
            &alg,
            &issuer,
            &validity,
            &subject,
            &spki,
            &exts,
        ]);
        let sig = issuer_key.sign(&tbs);
        CertificateDer::from(der_seq(&[&tbs, &alg, &der_bit_string(0, &sig.to_bytes())]))
    }

    pub struct TestCaLeaf {
        pub ca_cert: CertificateDer<'static>,
        pub leaf_cert: CertificateDer<'static>,
        pub leaf_key: PrivateKeyDer<'static>,
    }

    pub fn issue(
        ca_seed: [u8; 32],
        leaf_seed: [u8; 32],
        ca_cn: &str,
        dns_name: &str,
    ) -> TestCaLeaf {
        let ca_key = SigningKey::from_bytes(&ca_seed);
        let leaf_key = SigningKey::from_bytes(&leaf_seed);
        let ca_cert = sign_x509_v3(
            1,
            ca_cn,
            ca_cn,
            &ca_key,
            &ca_key,
            &[
                extension(
                    &[0x55, 0x1d, 0x13],
                    true,
                    &der_seq(&[&der_tlv(0x01, &[0xff])]),
                ),
                extension(&[0x55, 0x1d, 0x0f], true, &der_bit_string(2, &[0x04])),
            ],
        );
        let leaf_cert = sign_x509_v3(
            2,
            ca_cn,
            dns_name,
            &leaf_key,
            &ca_key,
            &[
                extension(&[0x55, 0x1d, 0x13], true, &der_seq(&[])),
                extension(&[0x55, 0x1d, 0x0f], true, &der_bit_string(7, &[0x80])),
                extension(
                    &[0x55, 0x1d, 0x25],
                    false,
                    &der_seq(&[&der_oid(&[0x2b, 0x06, 0x01, 0x05, 0x05, 0x07, 0x03, 0x01])]),
                ),
                extension(
                    &[0x55, 0x1d, 0x11],
                    false,
                    &der_seq(&[&der_tlv(0x82, dns_name.as_bytes())]),
                ),
            ],
        );
        TestCaLeaf {
            ca_cert,
            leaf_cert,
            leaf_key: ed25519_pkcs8(&leaf_key),
        }
    }
}

struct TestBootstrapAuthority {
    psk: [u8; 32],
    cloud_key: ed25519_dalek::SigningKey,
    allowed_controller: [u8; 32],
}

impl super::CloudIngressAuthority for TestBootstrapAuthority {
    fn bootstrap_responder(&self) -> Result<BootstrapResponder, super::IngressError> {
        BootstrapResponder::new(&self.psk)
    }

    fn installation_public_key(&self) -> Result<[u8; 32], super::IngressError> {
        Ok(self.cloud_key.verifying_key().to_bytes())
    }

    fn sign_channel_proof(
        &self,
        proof: &super::ChannelProof,
    ) -> Result<[u8; 64], super::IngressError> {
        use ed25519_dalek::Signer;
        Ok(self.cloud_key.sign(proof.as_bytes()).to_bytes())
    }

    fn authorize_bootstrap_controller(
        &self,
        key: &[u8; 32],
        _binding: &[u8; 32],
    ) -> Result<(), super::IngressError> {
        if key == &self.allowed_controller {
            Ok(())
        } else {
            Err(super::IngressError::Rejected)
        }
    }
}

struct TestEnrollmentHandler {
    ops: parking_lot::Mutex<Vec<super::EnrollmentOperation>>,
}

impl super::EnrollmentHandler for TestEnrollmentHandler {
    fn handle<'a>(
        &'a self,
        _session: &'a super::EnrollmentSession,
        op: super::EnrollmentOperation,
        payload: &'a [u8],
    ) -> super::EnrollmentResult<'a> {
        self.ops.lock().push(op);
        let reply = payload.to_vec();
        Box::pin(async move { Ok(reply) })
    }
}

struct TestEndpointAuthority {
    inner: std::sync::Arc<crate::account_mesh::tests::support::FakeAuthority>,
    next_seq: std::sync::atomic::AtomicU64,
}

impl crate::account_mesh::AccountMeshAuthority for TestEndpointAuthority {
    fn accept_endpoint_sequence<'a>(
        &'a self,
        membership: &'a crate::account_mesh::VerifiedMembership,
        sequence: u64,
        digest: [u8; 32],
    ) -> crate::account_mesh::BoxFuture<'a, Result<bool, crate::account_mesh::AdmissionRefusal>>
    {
        self.inner
            .accept_endpoint_sequence(membership, sequence, digest)
    }

    fn receive_registry_snapshot<'a>(
        &'a self,
        snapshot: &'a codeterm_entitlements::DetachedGrant,
    ) -> crate::account_mesh::BoxFuture<'a, Result<bool, crate::account_mesh::AdmissionRefusal>>
    {
        self.inner.receive_registry_snapshot(snapshot)
    }

    fn verify_membership(
        &self,
        token: &str,
        now: u64,
    ) -> Result<crate::account_mesh::VerifiedMembership, crate::account_mesh::AdmissionRefusal>
    {
        self.inner.verify_membership(token, now)
    }

    fn sign_possession(
        &self,
        transcript: &[u8; 32],
    ) -> Result<String, crate::account_mesh::AdmissionRefusal> {
        self.inner.sign_possession(transcript)
    }

    fn reserve_endpoint_sequence(
        &self,
        _epoch: u64,
    ) -> crate::account_mesh::BoxFuture<'_, Result<u64, crate::account_mesh::AdmissionRefusal>>
    {
        let seq = self
            .next_seq
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Box::pin(async move { Ok(seq) })
    }

    fn renew_cloud<'a>(
        &'a self,
        target: &'a crate::account_mesh::VerifiedMembership,
    ) -> crate::account_mesh::BoxFuture<
        'a,
        Result<
            (
                codeterm_entitlements::DetachedGrant,
                codeterm_entitlements::DetachedGrant,
            ),
            crate::account_mesh::AdmissionRefusal,
        >,
    > {
        self.inner.renew_cloud(target)
    }

    fn verify_key_possession(&self, key_id: &str, digest: &[u8; 32], signature: &str) -> bool {
        self.inner.verify_key_possession(key_id, digest, signature)
    }

    fn verify_possession(
        &self,
        membership: &crate::account_mesh::VerifiedMembership,
        transcript: &[u8; 32],
        signature: &str,
    ) -> bool {
        self.inner
            .verify_possession(membership, transcript, signature)
    }

    fn resolve_link_target(
        &self,
        installation_id: &str,
    ) -> crate::account_mesh::BoxFuture<
        '_,
        Result<crate::account_mesh::LinkTarget, crate::account_mesh::AdmissionRefusal>,
    > {
        self.inner.resolve_link_target(installation_id)
    }
}

struct TestRenewalReceiver {
    cloud: crate::MeshState,
    authority: std::sync::Arc<crate::account_mesh::tests::support::FakeAuthority>,
    cloud_id: String,
    calls: std::sync::atomic::AtomicUsize,
}

impl super::RenewalReceiver for TestRenewalReceiver {
    fn enrolled(&self) -> bool {
        true
    }

    fn apply<'a>(
        &'a self,
        _membership: &'a codeterm_entitlements::DetachedGrant,
        _registry: &'a codeterm_entitlements::DetachedGrant,
    ) -> crate::account_mesh::BoxFuture<'a, Result<(), super::IngressError>> {
        Box::pin(async move {
            use crate::account_mesh::tests::support::{membership, ACCOUNT, MEMBERSHIP_TTL, START};
            use crate::account_mesh::{AccountMeshState, AccountPresence};
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let members = self
                .authority
                .registry
                .lock()
                .keys()
                .chain(std::iter::once(&self.cloud_id))
                .map(|id| (id.clone(), (format!("key-{id}"), 1, 1)))
                .collect();
            self.cloud.lock().apply_account_state(AccountMeshState {
                registry_members: Some(members),
                account_epoch: 1,
                presence: AccountPresence::SignedIn {
                    account_id: ACCOUNT.into(),
                },
                membership: Some(membership(
                    ACCOUNT,
                    &self.cloud_id,
                    START + 3 * MEMBERSHIP_TTL,
                )),
                revoked_installations: Default::default(),
                unavailable_installations: Default::default(),
                plan_limit_locked: false,
            });
            Ok(())
        })
    }
}

struct TestDispatcher;

impl super::ReverseDispatcher for TestDispatcher {
    fn enabled(&self) -> bool {
        true
    }

    fn dispatch(
        &self,
        _request: crate::cloud_outbox::OuterWireRequest,
        _signed_token: String,
    ) -> crate::account_mesh::BoxFuture<'_, Result<Option<Vec<u8>>, super::IngressError>> {
        Box::pin(async { Ok(None) })
    }
}

async fn spawn_tls_proxy(
    backend: std::net::SocketAddr,
    leaf_cert: rustls_pki_types::CertificateDer<'static>,
    leaf_key: rustls_pki_types::PrivateKeyDer<'static>,
) -> (
    std::net::SocketAddr,
    std::sync::Arc<std::sync::atomic::AtomicUsize>,
    tokio::task::JoinHandle<()>,
) {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    let mut server_config = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_no_client_auth()
    .with_single_cert(vec![leaf_cert], leaf_key)
    .unwrap();
    server_config.alpn_protocols = vec![b"http/1.1".to_vec()];
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(server_config));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let alpn_hits = Arc::new(AtomicUsize::new(0));
    let hits = Arc::clone(&alpn_hits);
    let task = tokio::spawn(async move {
        loop {
            let Ok((inbound, _)) = listener.accept().await else {
                break;
            };
            let acceptor = acceptor.clone();
            let hits = Arc::clone(&hits);
            tokio::spawn(async move {
                let Ok(mut tls) = acceptor.accept(inbound).await else {
                    return;
                };
                if tls.get_ref().1.alpn_protocol() == Some(b"http/1.1".as_slice()) {
                    hits.fetch_add(1, Ordering::SeqCst);
                }
                let Ok(mut upstream) = tokio::net::TcpStream::connect(backend).await else {
                    return;
                };
                let _ = tokio::io::copy_bidirectional(&mut tls, &mut upstream).await;
            });
        }
    });
    (addr, alpn_hits, task)
}

#[tokio::test]
async fn provider_proxy_wss_runs_bootstrap_enrolled_publication_renewal_and_enforces_tls() {
    use crate::account_mesh::link::link_installation_with;
    use crate::account_mesh::tests::support::*;
    use ed25519_dalek::{Signer, SigningKey};
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
    use std::sync::Arc;

    const HOST: &str = "test-cloud.example.test";
    let trusted = test_pki::issue([11; 32], [12; 32], "domios-test-trusted-ca", HOST);
    let untrusted = test_pki::issue([21; 32], [22; 32], "domios-test-untrusted-ca", HOST);
    let mut root_store = rustls::RootCertStore::empty();
    root_store.add(trusted.ca_cert).unwrap();
    let roots = Arc::new(root_store);

    let psk = [9u8; 32];
    let controller_key = SigningKey::from_bytes(&[31; 32]);
    let cloud_bootstrap_key = SigningKey::from_bytes(&[32; 32]);
    let enrollment = Arc::new(TestEnrollmentHandler {
        ops: parking_lot::Mutex::new(Vec::new()),
    });
    let bootstrap_authority = Arc::new(TestBootstrapAuthority {
        psk,
        cloud_key: cloud_bootstrap_key,
        allowed_controller: controller_key.verifying_key().to_bytes(),
    });
    let (handle, ready) = super::start_with_proxy_host(
        "127.0.0.1:0".parse().unwrap(),
        Some(HOST.to_owned()),
        enrollment.clone(),
        bootstrap_authority,
    );
    let backend = ready.await.unwrap().bound_address();
    let (trusted_proxy, alpn_hits, trusted_task) =
        spawn_tls_proxy(backend, trusted.leaf_cert, trusted.leaf_key).await;
    let (untrusted_proxy, _, untrusted_task) =
        spawn_tls_proxy(backend, untrusted.leaf_cert, untrusted.leaf_key).await;

    let route = super::CloudRoute::ProviderProxy {
        host: HOST.to_owned(),
    };
    let clock = clock();
    let (home, cloud) = (machine(1, &clock), machine(2, &clock));
    cloud
        .mesh
        .lock()
        .bind_account_authority(Arc::new(TestEndpointAuthority {
            inner: Arc::clone(&cloud.authority),
            next_seq: AtomicU64::new(1),
        }));
    enroll_all(&[&home, &cloud]);
    home.sign_in(1, ACCOUNT, START + 4 * MEMBERSHIP_TTL);
    let mut readiness = std::pin::pin!(handle.enrolled_readiness());
    assert!(futures_util::poll!(readiness.as_mut()).is_pending());
    let receiver = Arc::new(TestRenewalReceiver {
        cloud: cloud.mesh.clone(),
        authority: Arc::clone(&cloud.authority),
        cloud_id: cloud.installation_id.clone(),
        calls: AtomicUsize::new(0),
    });
    assert!(handle.bind_enrolled(
        cloud.mesh.clone(),
        receiver.clone(),
        Arc::new(TestDispatcher)
    ));
    assert!(futures_util::poll!(readiness.as_mut()).is_pending());
    cloud.sign_in(1, ACCOUNT, START + MEMBERSHIP_TTL);
    readiness.await.unwrap();
    link_installation_with(
        &home.mesh,
        &Loopback::new(&[&home, &cloud]),
        &cloud.installation_id,
    )
    .await
    .unwrap();

    let trusted_override = super::dial::TestTlsOverride {
        target: trusted_proxy,
        roots: Arc::clone(&roots),
    };
    super::dial::TEST_TLS_OVERRIDE
        .scope(trusted_override, async {
            // 1. Noise_NNpsk0 bootstrap Discover + Apply over wss://
            let mut client = super::connect_enrollment(
                route.clone(),
                &psk,
                controller_key.verifying_key().to_bytes(),
                |proof| Ok(controller_key.sign(proof.as_bytes()).to_bytes()),
            )
            .await
            .unwrap();
            assert_eq!(
                client
                    .request(super::EnrollmentOperation::Discover, b"hello")
                    .await
                    .unwrap(),
                b"hello"
            );
            assert_eq!(
                client
                    .request(super::EnrollmentOperation::Apply, b"apply")
                    .await
                    .unwrap(),
                b"apply"
            );
            assert_eq!(
                *enrollment.ops.lock(),
                vec![
                    super::EnrollmentOperation::Discover,
                    super::EnrollmentOperation::Apply,
                ]
            );

            // 2. Enrolled channel + endpoint publication + gossip + DirectIngressStatus
            let channel =
                super::connect_enrolled(home.mesh.clone(), route.clone(), &cloud.installation_id)
                    .await
                    .unwrap();
            channel.publish_endpoint(&route).await.unwrap();
            assert_eq!(
                home.mesh
                    .lock()
                    .cloud_ingress_endpoint(&cloud.installation_id),
                Some(route.clone())
            );
            let status = home
                .mesh
                .lock()
                .peers_response()
                .direct_ingress
                .get(&cloud.installation_id)
                .cloned()
                .expect("provider_proxy peer exposed in direct_ingress status");
            assert_eq!(status.address, format!("wss://{HOST}"));

            // 3. Enrolled frame exchange (push_registry)
            let snapshot = codeterm_entitlements::DetachedGrant {
                payload: "registry-snapshot".into(),
                signature: "sig".into(),
                key_id: "issuer".into(),
                alg: "Ed25519".into(),
            };
            let channel =
                super::connect_enrolled(home.mesh.clone(), route.clone(), &cloud.installation_id)
                    .await
                    .unwrap();
            channel.push_registry(snapshot.clone()).await.unwrap();
            assert_eq!(cloud.authority.snapshots.lock().as_slice(), &[snapshot]);

            // 4. Renewal recovery over wss:// after cloud membership expires
            clock.store(START + 2 * MEMBERSHIP_TTL, Ordering::SeqCst);
            assert!(super::connect_enrolled(
                home.mesh.clone(),
                route.clone(),
                &cloud.installation_id,
            )
            .await
            .is_err());
            let grant = |payload: &str| codeterm_entitlements::DetachedGrant {
                payload: payload.into(),
                signature: "sig".into(),
                key_id: "issuer".into(),
                alg: "Ed25519".into(),
            };
            *home.authority.renewal.lock() = Some((grant("renewed"), grant("registry")));
            super::recover_expired_cloud(&home.mesh, route.clone(), &cloud.installation_id)
                .await
                .unwrap();
            assert_eq!(receiver.calls.load(Ordering::SeqCst), 1);
            let channel =
                super::connect_enrolled(home.mesh.clone(), route.clone(), &cloud.installation_id)
                    .await
                    .unwrap();
            assert_eq!(
                channel.peer_membership().unwrap().expires_at,
                START + 3 * MEMBERSHIP_TTL
            );

            // 5. Wrong hostname fails TLS verification with IngressError::TlsFailed
            let wrong_host = super::CloudRoute::ProviderProxy {
                host: "wrong-host.example.test".into(),
            };
            assert_eq!(
                super::connect_enrolled(home.mesh.clone(), wrong_host, &cloud.installation_id)
                    .await
                    .err(),
                Some(super::IngressError::TlsFailed)
            );
        })
        .await;

    // 6. Untrusted CA fails TLS verification with IngressError::TlsFailed
    let untrusted_override = super::dial::TestTlsOverride {
        target: untrusted_proxy,
        roots,
    };
    super::dial::TEST_TLS_OVERRIDE
        .scope(untrusted_override, async {
            assert_eq!(
                super::connect_enrolled(home.mesh.clone(), route.clone(), &cloud.installation_id)
                    .await
                    .err(),
                Some(super::IngressError::TlsFailed)
            );
        })
        .await;

    assert!(
        alpn_hits.load(Ordering::SeqCst) >= 5,
        "expected ALPN http/1.1 negotiation on trusted wss connections"
    );
    trusted_task.abort();
    untrusted_task.abort();
}

#[tokio::test]
async fn guest_derives_authoritative_host_from_platform_and_rejects_mismatched_host_header() {
    use crate::account_mesh::link::link_installation_with;
    use crate::account_mesh::tests::support::*;
    use ed25519_dalek::SigningKey;
    use std::sync::atomic::{AtomicU64, AtomicUsize};
    use std::sync::Arc;

    // Pure platform-env derivation checks.
    assert_eq!(
        super::listener::authoritative_proxy_host_from_vars(
            Some("fly_machines"),
            Some("domios-a1b2c3d4"),
        ),
        Some("domios-a1b2c3d4.fly.dev".into())
    );
    for (platform, app) in [
        (Some("timeweb"), Some("domios-a1b2c3d4")),
        (None, Some("domios-a1b2c3d4")),
        (Some("fly_machines"), None),
        (Some("fly_machines"), Some("")),
        (Some("fly_machines"), Some("BadApp")),
        (Some("fly_machines"), Some("evil.example.com")),
        (Some("fly_machines"), Some("127.0.0.1")),
    ] {
        assert_eq!(
            super::listener::authoritative_proxy_host_from_vars(platform, app),
            None,
            "expected None for platform={platform:?}, app={app:?}"
        );
    }

    const EXPECTED_HOST: &str = "domios-authoritative.fly.dev";
    const MISMATCHED_HOST: &str = "attacker.example.test";
    let mismatched_pki = test_pki::issue(
        [41; 32],
        [42; 32],
        "domios-test-mismatch-ca",
        MISMATCHED_HOST,
    );
    let mut root_store = rustls::RootCertStore::empty();
    root_store.add(mismatched_pki.ca_cert).unwrap();
    let roots = Arc::new(root_store);

    let enrollment = Arc::new(TestEnrollmentHandler {
        ops: parking_lot::Mutex::new(Vec::new()),
    });
    let bootstrap_authority = Arc::new(TestBootstrapAuthority {
        psk: [5u8; 32],
        cloud_key: SigningKey::from_bytes(&[43; 32]),
        allowed_controller: SigningKey::from_bytes(&[44; 32]).verifying_key().to_bytes(),
    });
    let (handle, ready) = super::start_with_proxy_host(
        "127.0.0.1:0".parse().unwrap(),
        Some(EXPECTED_HOST.to_owned()),
        enrollment.clone(),
        bootstrap_authority.clone(),
    );
    let backend = ready.await.unwrap().bound_address();
    let (proxy_addr, _, proxy_task) = spawn_tls_proxy(
        backend,
        mismatched_pki.leaf_cert.clone(),
        mismatched_pki.leaf_key.clone_key(),
    )
    .await;

    let clock = clock();
    let (home, cloud) = (machine(1, &clock), machine(2, &clock));
    cloud
        .mesh
        .lock()
        .bind_account_authority(Arc::new(TestEndpointAuthority {
            inner: Arc::clone(&cloud.authority),
            next_seq: AtomicU64::new(1),
        }));
    enroll_all(&[&home, &cloud]);
    home.sign_in(1, ACCOUNT, START + 4 * MEMBERSHIP_TTL);
    cloud.sign_in(1, ACCOUNT, START + MEMBERSHIP_TTL);
    link_installation_with(
        &home.mesh,
        &Loopback::new(&[&home, &cloud]),
        &cloud.installation_id,
    )
    .await
    .unwrap();
    let receiver = Arc::new(TestRenewalReceiver {
        cloud: cloud.mesh.clone(),
        authority: Arc::clone(&cloud.authority),
        cloud_id: cloud.installation_id.clone(),
        calls: AtomicUsize::new(0),
    });
    assert!(handle.bind_enrolled(
        cloud.mesh.clone(),
        receiver.clone(),
        Arc::new(TestDispatcher)
    ));

    // 1. TLS succeeds for MISMATCHED_HOST, but the guest listener rejects the mismatched HTTP Host.
    let mismatched_route = super::CloudRoute::ProviderProxy {
        host: MISMATCHED_HOST.to_owned(),
    };
    let tls_override = super::dial::TestTlsOverride {
        target: proxy_addr,
        roots: Arc::clone(&roots),
    };
    super::dial::TEST_TLS_OVERRIDE
        .scope(tls_override, async {
            assert!(
                super::connect_enrolled(
                    home.mesh.clone(),
                    mismatched_route.clone(),
                    &cloud.installation_id,
                )
                .await
                .is_err(),
                "guest listener must reject HTTP Host header that does not match authoritative host"
            );
        })
        .await;
    proxy_task.abort();

    // 2. Without authoritative platform host, Request::Endpoint never signs a client Host header.
    let (unconfigured_handle, unconfigured_ready) = super::start_with_proxy_host(
        "127.0.0.1:0".parse().unwrap(),
        None,
        enrollment,
        bootstrap_authority,
    );
    let unconfigured_backend = unconfigured_ready.await.unwrap().bound_address();
    let (unconfigured_proxy, _, unconfigured_task) = spawn_tls_proxy(
        unconfigured_backend,
        mismatched_pki.leaf_cert,
        mismatched_pki.leaf_key,
    )
    .await;
    assert!(unconfigured_handle.bind_enrolled(
        cloud.mesh.clone(),
        receiver,
        Arc::new(TestDispatcher)
    ));
    let unconfigured_override = super::dial::TestTlsOverride {
        target: unconfigured_proxy,
        roots,
    };
    super::dial::TEST_TLS_OVERRIDE
        .scope(unconfigured_override, async {
            let channel = super::connect_enrolled(
                home.mesh.clone(),
                mismatched_route.clone(),
                &cloud.installation_id,
            )
            .await
            .unwrap();
            assert!(
                channel.publish_endpoint(&mismatched_route).await.is_err(),
                "guest without authoritative platform host must never sign a client-supplied Host"
            );
        })
        .await;
    unconfigured_task.abort();
}

#[tokio::test]
async fn provider_proxy_dial_refuses_private_loopback_and_mixed_resolved_ips() {
    use std::net::SocketAddr;
    use std::sync::Arc;

    let route = super::CloudRoute::ProviderProxy {
        host: "domios-ssrf.fly.dev".into(),
    };

    for (label, resolved) in [
        (
            "loopback_v4",
            vec!["127.0.0.1:443".parse::<SocketAddr>().unwrap()],
        ),
        ("rfc1918_10", vec!["10.0.0.5:443".parse().unwrap()]),
        (
            "link_local_metadata",
            vec!["169.254.169.254:443".parse().unwrap()],
        ),
        ("rfc1918_172", vec!["172.16.0.1:443".parse().unwrap()]),
        ("rfc1918_192", vec!["192.168.1.1:443".parse().unwrap()]),
        ("cgnat", vec![SocketAddr::from(([100, 64, 0, 1], 443))]),
        ("loopback_v6", vec!["[::1]:443".parse().unwrap()]),
        (
            "v4_mapped_loopback",
            vec!["[::ffff:127.0.0.1]:443".parse().unwrap()],
        ),
        (
            "mixed_public_and_private",
            vec![
                "8.8.8.8:443".parse().unwrap(),
                "10.0.0.1:443".parse().unwrap(),
            ],
        ),
        ("non_443_port", vec!["8.8.8.8:7687".parse().unwrap()]),
    ] {
        let resolver: super::dial::TestResolverFn =
            Arc::new(move |_host, _port| Ok(resolved.clone()));
        let err = super::dial::TEST_RESOLVER
            .scope(resolver, super::dial::open(&route, "/cloud/enrolled/v1"))
            .await
            .err();
        assert_eq!(
            err,
            Some(super::IngressError::Rejected),
            "expected Rejected for {label}"
        );
    }

    let empty_resolver: super::dial::TestResolverFn = Arc::new(|_host, _port| Ok(Vec::new()));
    assert_eq!(
        super::dial::TEST_RESOLVER
            .scope(
                empty_resolver,
                super::dial::open(&route, "/cloud/enrolled/v1")
            )
            .await
            .err(),
        Some(super::IngressError::Unavailable)
    );

    // Pure vetting accepts all-public port-443 results (both IPv4 and IPv6).
    let vetted = super::dial::vet_resolved_provider_proxy_addrs(&[
        "8.8.8.8:443".parse().unwrap(),
        "[2606:2800:220:1:248:1893:25c8:1946]:443".parse().unwrap(),
    ])
    .unwrap();
    assert_eq!(vetted.len(), 2);
}

#[tokio::test]
async fn enrollment_wait_bounds_cover_cold_fly_boot_and_fail_when_stalled() {
    use ed25519_dalek::{Signer, SigningKey};
    use std::sync::Arc;
    use std::time::Duration;

    assert!(
        super::ENROLLMENT_CONNECT_TIMEOUT >= Duration::from_secs(180),
        "enrollment connect failure bound must cover cold Fly boot (>= 180 s)"
    );
    assert!(
        super::ENROLLMENT_REQUEST_TIMEOUT >= Duration::from_secs(180),
        "enrollment request failure bound must cover guest startup wait (>= 180 s)"
    );
    assert!(
        super::BOOTSTRAP_SESSION_TIMEOUT >= super::ENROLLMENT_CONNECT_TIMEOUT,
        "guest bootstrap session bound must not expire before the controller connect bound"
    );

    // A silent remote that accepts TLS/TCP without completing the WebSocket/Noise handshake
    // must fail cleanly with IngressError::Unavailable at the configured failure bound.
    const HOST: &str = "stalled-boot.example.test";
    let pki = test_pki::issue([51; 32], [52; 32], "domios-test-stall-ca", HOST);
    let mut root_store = rustls::RootCertStore::empty();
    root_store.add(pki.ca_cert).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let stalled_addr = listener.local_addr().unwrap();
    let stall_task = tokio::spawn(async move {
        if let Ok((stream, _)) = listener.accept().await {
            let _hold = stream;
            std::future::pending::<()>().await;
        }
    });

    let controller_key = SigningKey::from_bytes(&[53; 32]);
    let route = super::CloudRoute::ProviderProxy {
        host: HOST.to_owned(),
    };
    let tls_override = super::dial::TestTlsOverride {
        target: stalled_addr,
        roots: Arc::new(root_store),
    };
    let outcome = super::dial::TEST_TLS_OVERRIDE
        .scope(
            tls_override,
            super::controller::connect_enrollment_within(
                route,
                &[7u8; 32],
                controller_key.verifying_key().to_bytes(),
                Duration::from_millis(50),
                |proof| Ok(controller_key.sign(proof.as_bytes()).to_bytes()),
            ),
        )
        .await;
    assert_eq!(outcome.err(), Some(super::IngressError::Unavailable));
    stall_task.abort();
}
