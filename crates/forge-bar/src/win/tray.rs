//! Icône de notification : menu de secours quand la barre est masquée, et bulles d'alerte.

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_ERROR, NIIF_INFO, NIIF_WARNING, NIM_ADD,
    NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::{LoadIconW, IDI_APPLICATION, WM_APP};

pub const WM_TRAY: u32 = WM_APP + 5;

/// Texte UTF-16 terminé par un zéro, rempli dans un tableau de taille fixe. Renvoyé par valeur : sur x86
/// NOTIFYICONDATAW est compacte (alignement 1) et on ne peut pas en emprunter les champs.
fn wide<const N: usize>(s: &str) -> [u16; N] {
    let mut out = [0u16; N];
    for (d, c) in out.iter_mut().zip(s.encode_utf16().take(N - 1)) {
        *d = c;
    }
    out
}

/// Icône 3D de l'application (ressource n° 1 de l'exécutable), à défaut l'icône générique de Windows.
fn app_icon() -> windows::Win32::UI::WindowsAndMessaging::HICON {
    unsafe {
        let module = windows::Win32::System::LibraryLoader::GetModuleHandleW(None).unwrap_or_default();
        LoadIconW(module, windows::core::PCWSTR(std::ptr::without_provenance(1)))
            .or_else(|_| LoadIconW(None, IDI_APPLICATION))
            .unwrap_or_default()
    }
}

fn data(hwnd: HWND) -> NOTIFYICONDATAW {
    let mut d = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: 1,
        uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
        uCallbackMessage: WM_TRAY,
        hIcon: app_icon(),
        ..Default::default()
    };
    d.szTip = wide("Forge Bar");
    d
}

pub fn add(hwnd: HWND) {
    unsafe {
        let _ = Shell_NotifyIconW(NIM_ADD, &data(hwnd));
    }
}

pub fn remove(hwnd: HWND) {
    unsafe {
        let _ = Shell_NotifyIconW(NIM_DELETE, &data(hwnd));
    }
}

/// Bulle de notification Windows.
pub fn balloon(hwnd: HWND, title: &str, body: &str, critical: bool) {
    notify(hwnd, title, body, if critical { NIIF_ERROR } else { NIIF_WARNING });
}

/// Bulle d'information simple (résultat d'un test, confirmation).
pub fn info(hwnd: HWND, title: &str, body: &str) {
    notify(hwnd, title, body, NIIF_INFO);
}

fn notify(hwnd: HWND, title: &str, body: &str, flags: windows::Win32::UI::Shell::NOTIFY_ICON_INFOTIP_FLAGS) {
    let mut d = data(hwnd);
    d.uFlags = NIF_INFO;
    d.szInfoTitle = wide(title);
    d.szInfo = wide(body);
    d.dwInfoFlags = flags;
    unsafe {
        let _ = Shell_NotifyIconW(NIM_MODIFY, &d);
    }
}
