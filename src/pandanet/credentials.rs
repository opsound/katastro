use super::Credentials;
use crate::Result;
/// macOS generic-password item, scoped to Katastro and the Pandanet username.
pub struct Keychain {
    service: String,
}
impl Keychain {
    pub fn new(service: &str) -> Self {
        Self {
            service: service.into(),
        }
    }
}
impl Default for Keychain {
    fn default() -> Self {
        Self::new("com.katastro.pandanet")
    }
}
#[cfg(target_os = "macos")]
impl Credentials for Keychain {
    fn load(&self, account: &str) -> Result<Option<String>> {
        match security_framework::passwords::get_generic_password(&self.service, account) {
            Ok(bytes) => Ok(Some(String::from_utf8(bytes)?)),
            Err(error) if error.code() == -25300 => Ok(None), // errSecItemNotFound
            Err(error) => {
                Err(format!("Could not read macOS Keychain (status {})", error.code()).into())
            }
        }
    }
    fn save(&self, account: &str, password: &str) -> Result<()> {
        security_framework::passwords::set_generic_password(
            &self.service,
            account,
            password.as_bytes(),
        )
        .map_err(|e| {
            format!(
                "Could not save login in macOS Keychain (status {})",
                e.code()
            )
            .into()
        })
    }
    fn forget(&self, account: &str) -> Result<()> {
        match security_framework::passwords::delete_generic_password(&self.service, account) {
            Ok(()) => Ok(()),
            Err(error) if error.code() == -25300 => Ok(()),
            Err(error) => Err(format!(
                "Could not remove login from macOS Keychain (status {})",
                error.code()
            )
            .into()),
        }
    }
}
#[cfg(not(target_os = "macos"))]
impl Credentials for Keychain {
    fn load(&self, _account: &str) -> Result<Option<String>> {
        let _ = &self.service;
        Ok(None)
    }
    fn save(&self, _account: &str, _password: &str) -> Result<()> {
        Err("Remember login requires macOS Keychain".into())
    }
    fn forget(&self, _account: &str) -> Result<()> {
        Ok(())
    }
}
