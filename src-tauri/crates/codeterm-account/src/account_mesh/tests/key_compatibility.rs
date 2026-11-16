use crate::account_mesh::{verify_possession, InstallationKey};
use crate::Secret;

#[test]
fn stored_seed_and_possession_signature_are_byte_compatible() {
    let stored = Secret::new("BwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwc");
    let key = InstallationKey::from_secret(&stored).unwrap();
    let possession = concat!(
        "MCowBQYDK2VwAyEA6kpsY-KcUgq-9VB7Ey7F-ZVHdq6-vnuSQh7qaRRG0iw.",
        "pf64vNSALiIYfogxPinhQxiL-b8FrMVVKX8Hu_QzoKWz8F6vgQva5XiFYr5xUUo6K85-1_RxFS-Y5fVorrAbAg"
    );
    assert_eq!(key.to_secret().expose(), stored.expose());
    assert_eq!(key.sign_possession(&[9; 32]), possession);
    assert!(verify_possession(&key.key_id(), &[9; 32], possession));
    assert!(!verify_possession(&key.key_id(), &[8; 32], possession));
    assert!(!verify_possession(&"0".repeat(64), &[9; 32], possession));
}
