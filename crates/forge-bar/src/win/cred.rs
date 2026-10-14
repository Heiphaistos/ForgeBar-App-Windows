//! Clé Admin API d'Anthropic, lue dans le Gestionnaire d'identification Windows.
//! Elle y est écrite par `forge-settings admin-key set`, jamais dans `config.toml`.

use windows::core::HSTRING;
use windows::Win32::Security::Credentials::{CredFree, CredReadW, CREDENTIALW, CRED_TYPE_GENERIC};

pub fn read_admin_key() -> Result<String, String> {
    unsafe {
        let mut cred: *mut CREDENTIALW = std::ptr::null_mut();
        CredReadW(&HSTRING::from(forge_config::ADMIN_KEY_TARGET), CRED_TYPE_GENERIC, 0, &mut cred)
            .map_err(|_| "clé Admin API absente (forge-settings admin-key set)".to_string())?;
        let c = &*cred;
        let bytes = std::slice::from_raw_parts(c.CredentialBlob, c.CredentialBlobSize as usize);
        let key = String::from_utf8_lossy(bytes).trim().to_string();
        CredFree(cred as _);
        if key.is_empty() {
            Err("clé Admin API vide".into())
        } else {
            Ok(key)
        }
    }
}
