//! Presse-papiers texte : lecture à chaque changement et réécriture d'une entrée de l'historique.
//! Le contenu marqué « à ne pas surveiller » par les gestionnaires de mots de passe est ignoré.

use windows::core::w;
use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL, HWND};
use windows::Win32::System::DataExchange::{
    AddClipboardFormatListener, CloseClipboard, EmptyClipboard, GetClipboardData, IsClipboardFormatAvailable,
    OpenClipboard, RegisterClipboardFormatW, RemoveClipboardFormatListener, SetClipboardData,
};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};

const CF_UNICODETEXT: u32 = 13;
/// Plus de 64 000 caractères : ce n'est pas du texte qu'on retrouve dans un historique.
const MAX_CHARS: usize = 64_000;

pub fn listen(main: HWND, on: bool) {
    unsafe {
        if on {
            let _ = AddClipboardFormatListener(main);
        } else {
            let _ = RemoveClipboardFormatListener(main);
        }
    }
}

fn open(owner: HWND) -> bool {
    for _ in 0..5 {
        if unsafe { OpenClipboard(owner) }.is_ok() {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    false
}

/// Texte courant, ou `None` (vide, pas du texte, ou exclu de la surveillance).
pub fn read_text(owner: HWND) -> Option<String> {
    if !open(owner) {
        return None;
    }
    let text = unsafe {
        let excluded = RegisterClipboardFormatW(w!("ExcludeClipboardContentFromMonitorProcessing"));
        if (excluded != 0 && IsClipboardFormatAvailable(excluded).is_ok())
            || IsClipboardFormatAvailable(CF_UNICODETEXT).is_err()
        {
            None
        } else {
            GetClipboardData(CF_UNICODETEXT).ok().and_then(|h| {
                let g = HGLOBAL(h.0);
                let p = GlobalLock(g) as *const u16;
                if p.is_null() {
                    return None;
                }
                let mut len = 0;
                while len < MAX_CHARS && *p.add(len) != 0 {
                    len += 1;
                }
                let s = String::from_utf16_lossy(std::slice::from_raw_parts(p, len));
                let _ = GlobalUnlock(g);
                Some(s)
            })
        }
    };
    unsafe {
        let _ = CloseClipboard();
    }
    text.filter(|t| !t.trim().is_empty())
}

pub fn write_text(owner: HWND, text: &str) -> bool {
    if !open(owner) {
        return false;
    }
    let wide: Vec<u16> = text.encode_utf16().chain([0]).collect();
    let ok = unsafe {
        let _ = EmptyClipboard();
        match GlobalAlloc(GMEM_MOVEABLE, wide.len() * 2) {
            Ok(g) => {
                let p = GlobalLock(g) as *mut u16;
                if p.is_null() {
                    let _ = GlobalFree(g);
                    false
                } else {
                    std::ptr::copy_nonoverlapping(wide.as_ptr(), p, wide.len());
                    let _ = GlobalUnlock(g);
                    // Le système possède la mémoire dès que SetClipboardData réussit.
                    let set = SetClipboardData(CF_UNICODETEXT, HANDLE(g.0)).is_ok();
                    if !set {
                        let _ = GlobalFree(g);
                    }
                    set
                }
            }
            Err(_) => false,
        }
    };
    unsafe {
        let _ = CloseClipboard();
    }
    ok
}
