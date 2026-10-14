//! Icône de notification : menu de secours quand la barre est masquée, et bulles d'alerte.

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_ERROR, NIIF_WARNING, NIM_ADD, NIM_DELETE,
    NIM_MODIFY, NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::{LoadIconW, IDI_APPLICATION, WM_APP};

pub const WM_TRAY: u32 = WM_APP + 5;

fn fill<const N: usize>(dst: &mut [u16; N], s: &str) {
    for (d, c) in dst.iter_mut().zip(s.encode_utf16().chain([0])) {
        *d = c;
    }
    dst[N - 1] = 0;
}

fn data(hwnd: HWND) -> NOTIFYICONDATAW {
    let mut d = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: 1,
        uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
        uCallbackMessage: WM_TRAY,
        hIcon: unsafe { LoadIconW(None, IDI_APPLICATION) }.unwrap_or_default(),
        ..Default::default()
    };
    fill(&mut d.szTip, "Forge Bar");
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
    let mut d = data(hwnd);
    d.uFlags = NIF_INFO;
    fill(&mut d.szInfoTitle, title);
    fill(&mut d.szInfo, body);
    d.dwInfoFlags = if critical { NIIF_ERROR } else { NIIF_WARNING };
    unsafe {
        let _ = Shell_NotifyIconW(NIM_MODIFY, &d);
    }
}
