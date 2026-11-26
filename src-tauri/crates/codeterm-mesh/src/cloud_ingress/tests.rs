use super::authentication::BootstrapResponder;
use super::valid_endpoint;

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
fn provider_origin_is_canonical_https_metadata() {
    let header = "x-preview-token";
    assert!(valid_endpoint("https://cloud.example.com:8443", header));
    for origin in [
        "http://cloud.example.com",
        "https://127.0.0.1",
        "https://10.0.0.1",
        "https://[::1]",
        "https://host.ts.net",
        "https://service.localhost",
        "https://localhost",
        "https://example.com/other",
        "https://user@example.com",
        "https://example.com?token=x",
        "https://example.com#fragment",
    ] {
        assert!(!valid_endpoint(origin, header));
    }
    for header in [
        "authorization",
        "host",
        "x-preview\r\nvalue",
        "x-forwarded-for",
    ] {
        assert!(!valid_endpoint("https://cloud.example.com", header));
    }
}
