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
    for address in [
        "127.0.0.1:7687",
        "10.0.0.1:7687",
        "100.64.1.2:7687",
        "8.8.8.8:443",
    ] {
        assert!(!super::direct_address_allowed(address.parse().unwrap()));
    }
    assert!(super::direct_address_allowed(
        "8.8.8.8:7687".parse().unwrap()
    ));
}
