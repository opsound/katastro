#![cfg(target_os = "macos")]
use katastro::pandanet::{Credentials, Keychain};
#[test]
#[ignore = "Uses the macOS login Keychain with a disposable test item; may request Keychain access"]
fn native_keychain_remembers_updates_and_forgets_a_disposable_login() {
    let service = format!("com.katastro.test.{}", std::process::id());
    let vault = Keychain::new(&service);
    struct Cleanup(String);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = Keychain::new(&self.0).forget("synthetic-account");
        }
    }
    let _cleanup = Cleanup(service.clone());
    assert!(vault.load("synthetic-account").unwrap().is_none());
    vault
        .save("synthetic-account", "disposable-secret-one")
        .unwrap();
    assert_eq!(
        Keychain::new(&service)
            .load("synthetic-account")
            .unwrap()
            .as_deref(),
        Some("disposable-secret-one")
    );
    vault
        .save("synthetic-account", "disposable-secret-two")
        .unwrap();
    assert_eq!(
        vault.load("synthetic-account").unwrap().as_deref(),
        Some("disposable-secret-two")
    );
    vault.forget("synthetic-account").unwrap();
    assert!(vault.load("synthetic-account").unwrap().is_none());
    vault.forget("synthetic-account").unwrap();
}
