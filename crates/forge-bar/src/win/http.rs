//! Client HTTPS minimal sur WinHTTP : aucune dépendance, proxy et certificats du système.
//! Utilisé pour la météo (Open-Meteo) et l'Admin API d'Anthropic. Toujours appelé depuis un fil de fond.

use std::ffi::c_void;
use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::Networking::WinHttp::*;

const MAX_BODY: usize = 2 * 1024 * 1024;

/// Poignée WinHTTP fermée à la destruction.
struct Handle(*mut c_void);

impl Drop for Handle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                let _ = WinHttpCloseHandle(self.0);
            }
        }
    }
}

fn check(h: *mut c_void, what: &str) -> Result<Handle, String> {
    if h.is_null() {
        Err(format!("{what} : {}", windows::core::Error::from_win32()))
    } else {
        Ok(Handle(h))
    }
}

/// GET en HTTPS. Renvoie le corps pour tout statut 2xx, sinon une erreur avec le statut
/// et le début du corps (l'API Anthropic y met le message d'erreur).
pub fn get(host: &str, path: &str, headers: &[(&str, &str)]) -> Result<String, String> {
    unsafe {
        let agent = HSTRING::from(concat!("ForgeBar/", env!("CARGO_PKG_VERSION")));
        let session = check(
            WinHttpOpen(&agent, WINHTTP_ACCESS_TYPE_DEFAULT_PROXY, PCWSTR::null(), PCWSTR::null(), 0),
            "ouverture",
        )?;
        WinHttpSetTimeouts(session.0, 8000, 8000, 10000, 15000).map_err(|e| e.to_string())?;
        let connect =
            check(WinHttpConnect(session.0, &HSTRING::from(host), INTERNET_DEFAULT_HTTPS_PORT, 0), "connexion")?;
        let request = check(
            WinHttpOpenRequest(
                connect.0,
                w!("GET"),
                &HSTRING::from(path),
                PCWSTR::null(),
                PCWSTR::null(),
                std::ptr::null(),
                WINHTTP_FLAG_SECURE,
            ),
            "requête",
        )?;
        let block: String = headers.iter().map(|(k, v)| format!("{k}: {v}\r\n")).collect();
        if !block.is_empty() {
            let wide: Vec<u16> = block.encode_utf16().collect();
            WinHttpAddRequestHeaders(request.0, &wide, WINHTTP_ADDREQ_FLAG_ADD).map_err(|e| e.to_string())?;
        }
        WinHttpSendRequest(request.0, None, None, 0, 0, 0).map_err(|e| format!("envoi : {e}"))?;
        WinHttpReceiveResponse(request.0, std::ptr::null_mut()).map_err(|e| format!("réponse : {e}"))?;

        let mut status = 0u32;
        let mut len = std::mem::size_of::<u32>() as u32;
        WinHttpQueryHeaders(
            request.0,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            PCWSTR::null(),
            Some(&mut status as *mut u32 as *mut c_void),
            &mut len,
            std::ptr::null_mut(),
        )
        .map_err(|e| format!("statut : {e}"))?;

        let mut body = Vec::new();
        loop {
            let mut avail = 0u32;
            WinHttpQueryDataAvailable(request.0, &mut avail).map_err(|e| format!("lecture : {e}"))?;
            if avail == 0 || body.len() >= MAX_BODY {
                break;
            }
            let mut chunk = vec![0u8; avail.min(64 * 1024) as usize];
            let mut read = 0u32;
            WinHttpReadData(request.0, chunk.as_mut_ptr() as *mut c_void, chunk.len() as u32, &mut read)
                .map_err(|e| format!("lecture : {e}"))?;
            if read == 0 {
                break;
            }
            body.extend_from_slice(&chunk[..read as usize]);
        }
        let text = String::from_utf8_lossy(&body).into_owned();
        if (200..300).contains(&status) {
            Ok(text)
        } else {
            let snippet: String = text.chars().take(200).collect();
            Err(format!("HTTP {status} {snippet}"))
        }
    }
}
