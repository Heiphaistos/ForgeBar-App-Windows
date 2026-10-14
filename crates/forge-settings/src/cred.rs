//! Clé Admin API Anthropic dans le Gestionnaire d'identification Windows, et octets aléatoires de l'OS.
//! La clé n'est jamais relue vers l'interface : seul `exists` est exposé.

use forge_config::ADMIN_KEY_TARGET;

/// Une clé Admin valide commence par `sk-ant-admin`, sans espace ni caractère de contrôle.
pub fn validate_admin_key(key: &str) -> Result<(), String> {
    let k = key.trim();
    if !k.starts_with("sk-ant-admin") {
        return Err("la clé Admin doit commencer par « sk-ant-admin »".into());
    }
    if k.len() > 512 || k.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err("clé Admin invalide (espaces ou caractères interdits)".into());
    }
    Ok(())
}

#[cfg(windows)]
mod imp {
    use super::ADMIN_KEY_TARGET;
    use windows::core::{PCWSTR, PWSTR};
    use windows::Win32::Foundation::{ERROR_NOT_FOUND, HANDLE, WIN32_ERROR};
    use windows::Win32::Security::Credentials::{
        CredDeleteW, CredFree, CredReadW, CredWriteW, CREDENTIALW, CRED_FLAGS, CRED_PERSIST_LOCAL_MACHINE,
        CRED_TYPE_GENERIC,
    };
    use windows::Win32::Security::Cryptography::{BCryptGenRandom, BCRYPT_USE_SYSTEM_PREFERRED_RNG};
    use windows::Win32::System::Console::{
        GetConsoleMode, GetStdHandle, SetConsoleMode, CONSOLE_MODE, ENABLE_ECHO_INPUT, STD_INPUT_HANDLE,
    };

    fn target() -> Vec<u16> {
        ADMIN_KEY_TARGET.encode_utf16().chain(std::iter::once(0)).collect()
    }

    pub fn store(key: &str) -> Result<(), String> {
        let mut t = target();
        let mut blob = key.trim().as_bytes().to_vec();
        let cred = CREDENTIALW {
            Flags: CRED_FLAGS(0),
            Type: CRED_TYPE_GENERIC,
            TargetName: PWSTR(t.as_mut_ptr()),
            CredentialBlobSize: blob.len() as u32,
            CredentialBlob: blob.as_mut_ptr(),
            Persist: CRED_PERSIST_LOCAL_MACHINE,
            ..Default::default()
        };
        // SAFETY: `cred` pointe vers `t` et `blob`, vivants pendant l'appel.
        let r = unsafe { CredWriteW(&cred, 0) };
        blob.fill(0);
        r.map_err(|e| format!("écriture dans le Gestionnaire d'identification impossible : {e}"))
    }

    pub fn exists() -> bool {
        let t = target();
        let mut out = std::ptr::null_mut();
        // SAFETY: `out` reçoit un bloc alloué par Windows, libéré par CredFree.
        unsafe {
            if CredReadW(PCWSTR(t.as_ptr()), CRED_TYPE_GENERIC, 0, &mut out).is_err() {
                return false;
            }
            CredFree(out as *const _);
        }
        true
    }

    pub fn clear() -> Result<(), String> {
        let t = target();
        // SAFETY: chaîne terminée par 0, vivante pendant l'appel.
        match unsafe { CredDeleteW(PCWSTR(t.as_ptr()), CRED_TYPE_GENERIC, 0) } {
            Ok(()) => Ok(()),
            Err(e) if e.code() == WIN32_ERROR(ERROR_NOT_FOUND.0).to_hresult() => Ok(()),
            Err(e) => Err(format!("suppression impossible : {e}")),
        }
    }

    /// Coupe l'écho de la console tant qu'il vit (sans effet si stdin est un tube).
    pub struct EchoOff(Option<(HANDLE, CONSOLE_MODE)>);

    impl EchoOff {
        pub fn new() -> Self {
            // SAFETY: appels console simples sur le handle stdin du processus.
            unsafe {
                let Ok(h) = GetStdHandle(STD_INPUT_HANDLE) else { return Self(None) };
                let mut mode = CONSOLE_MODE(0);
                if GetConsoleMode(h, &mut mode).is_err() {
                    return Self(None);
                }
                let _ = SetConsoleMode(h, CONSOLE_MODE(mode.0 & !ENABLE_ECHO_INPUT.0));
                Self(Some((h, mode)))
            }
        }
    }

    impl Drop for EchoOff {
        fn drop(&mut self) {
            if let Some((h, mode)) = self.0 {
                // SAFETY: restaure le mode d'origine.
                unsafe {
                    let _ = SetConsoleMode(h, mode);
                }
            }
        }
    }

    pub fn random(buf: &mut [u8]) -> Result<(), String> {
        // SAFETY: tampon valide ; RNG système sans handle.
        let st = unsafe { BCryptGenRandom(None, buf, BCRYPT_USE_SYSTEM_PREFERRED_RNG) };
        if st.0 == 0 {
            Ok(())
        } else {
            Err(format!("BCryptGenRandom a échoué ({:#x})", st.0))
        }
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn store(_: &str) -> Result<(), String> {
        Err("Gestionnaire d'identification disponible sous Windows uniquement".into())
    }
    pub fn exists() -> bool {
        false
    }
    pub fn clear() -> Result<(), String> {
        Ok(())
    }
    pub fn random(buf: &mut [u8]) -> Result<(), String> {
        use std::io::Read;
        std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(buf)).map_err(|e| e.to_string())
    }
}

/// Valide puis enregistre la clé.
pub fn set_admin_key(key: &str) -> Result<(), String> {
    validate_admin_key(key)?;
    imp::store(key)
}
pub use imp::{clear as clear_admin_key, exists as admin_key_exists, random};

/// Lit une ligne sur stdin ; en console Windows l'écho est coupé pour ne pas afficher la clé.
pub fn read_secret(prompt: &str) -> Result<String, String> {
    #[cfg(windows)]
    let _echo = imp::EchoOff::new();
    eprint!("{prompt}");
    let mut line = String::new();
    std::io::stdin().read_line(&mut line).map_err(|e| e.to_string())?;
    eprintln!();
    Ok(line.trim().to_string())
}

/// `n` octets aléatoires de l'OS, en hexadécimal.
pub fn random_hex(n: usize) -> Result<String, String> {
    let mut b = vec![0u8; n];
    random(&mut b)?;
    Ok(b.iter().map(|x| format!("{x:02x}")).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn admin_key_validation() {
        assert!(validate_admin_key("sk-ant-admin01-abc").is_ok());
        assert!(validate_admin_key("sk-ant-api03-abc").is_err());
        assert!(validate_admin_key("sk-ant-admin a").is_err());
        assert!(validate_admin_key("").is_err());
    }

    #[test]
    fn random_is_hex_and_differs() {
        let (a, b) = (random_hex(16).unwrap(), random_hex(16).unwrap());
        assert_eq!(a.len(), 32);
        assert_ne!(a, b);
    }
}
