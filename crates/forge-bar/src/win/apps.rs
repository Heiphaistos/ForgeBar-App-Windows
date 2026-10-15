//! Fenêtres ouvertes (comme la barre des tâches) : liste stable et icônes en pixels BGRA 32 x 32.

use crate::widgets::AppWin;
use std::collections::HashMap;
use std::sync::Arc;
use windows::Win32::Foundation::{BOOL, HWND, LPARAM, WPARAM};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
    DIB_RGB_COLORS,
};
use windows::Win32::UI::WindowsAndMessaging::*;

pub const SIZE: i32 = 32;
/// Au-delà, les miniatures prendraient toute la barre.
pub const MAX_APPS: usize = 10;

#[derive(Default)]
pub struct Windows {
    /// Ordre d'apparition : une fenêtre garde sa place tant qu'elle existe.
    order: Vec<isize>,
    icons: HashMap<isize, (isize, Arc<Vec<u8>>)>,
}

unsafe extern "system" fn collect(h: HWND, data: LPARAM) -> BOOL {
    let list = &mut *(data.0 as *mut Vec<HWND>);
    if is_task_window(h) {
        list.push(h);
    }
    BOOL(1)
}

fn is_task_window(h: HWND) -> bool {
    unsafe {
        if !IsWindowVisible(h).as_bool() || GetWindowTextLengthW(h) == 0 {
            return false;
        }
        let ex = GetWindowLongW(h, GWL_EXSTYLE) as u32;
        let owned = GetWindow(h, GW_OWNER).is_ok_and(|o| !o.is_invalid());
        if (ex & WS_EX_TOOLWINDOW.0 != 0 || owned) && ex & WS_EX_APPWINDOW.0 == 0 {
            return false;
        }
        // Fenêtres UWP fantômes (cloaked) : visibles pour Windows mais pas pour l'utilisateur.
        let mut cloaked = 0u32;
        let _ = DwmGetWindowAttribute(h, DWMWA_CLOAKED, &mut cloaked as *mut _ as _, 4);
        let mut class = [0u16; 32];
        let n = GetClassNameW(h, &mut class) as usize;
        cloaked == 0 && String::from_utf16_lossy(&class[..n]) != "Progman"
    }
}

fn title(h: HWND) -> String {
    let mut buf = [0u16; 256];
    let n = unsafe { GetWindowTextW(h, &mut buf) } as usize;
    String::from_utf16_lossy(&buf[..n])
}

fn icon_handle(h: HWND) -> Option<HICON> {
    unsafe {
        for kind in [2usize, 1, 0] {
            let mut out = 0usize;
            let ok = SendMessageTimeoutW(
                h,
                WM_GETICON,
                WPARAM(kind),
                LPARAM(0),
                SMTO_ABORTIFHUNG | SMTO_BLOCK,
                40,
                Some(&mut out),
            );
            if ok.0 != 0 && out != 0 {
                return Some(HICON(out as _));
            }
        }
        [GCLP_HICON, GCLP_HICONSM].iter().find_map(|i| {
            #[cfg(target_pointer_width = "64")]
            let v = GetClassLongPtrW(h, *i);
            // GetClassLongPtrW n'existe pas en 32 bits : GetClassLongW la remplace.
            #[cfg(target_pointer_width = "32")]
            let v = GetClassLongW(h, *i) as usize;
            (v != 0).then_some(HICON(v as _))
        })
    }
}

/// Dessine l'icône dans une image 32 bits : alpha préservé, résultat prémultiplié comme Direct2D l'attend.
fn rasterize(icon: HICON) -> Option<Vec<u8>> {
    unsafe {
        let dc = CreateCompatibleDC(None);
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: SIZE,
                biHeight: -SIZE,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        let Ok(bmp) = CreateDIBSection(dc, &bmi, DIB_RGB_COLORS, &mut bits, None, 0) else {
            let _ = DeleteDC(dc);
            return None;
        };
        let old = SelectObject(dc, bmp);
        let drawn = DrawIconEx(dc, 0, 0, icon, SIZE, SIZE, 0, None, DI_NORMAL).is_ok();
        let mut px = std::slice::from_raw_parts(bits as *const u8, (SIZE * SIZE * 4) as usize).to_vec();
        SelectObject(dc, old);
        let _ = DeleteObject(bmp);
        let _ = DeleteDC(dc);
        if !drawn {
            return None;
        }
        // Anciennes icônes sans canal alpha : tout pixel dessiné devient opaque.
        if px.chunks_exact(4).all(|p| p[3] == 0) {
            for p in px.chunks_exact_mut(4) {
                if p[..3].iter().any(|c| *c != 0) {
                    p[3] = 255;
                }
            }
        }
        Some(px)
    }
}

impl Windows {
    /// Liste courante ; `foreground` marque la fenêtre active.
    pub fn list(&mut self) -> Vec<AppWin> {
        let mut found: Vec<HWND> = Vec::new();
        unsafe {
            let _ = EnumWindows(Some(collect), LPARAM(&mut found as *mut _ as isize));
        }
        let ids: Vec<isize> = found.iter().map(|h| h.0 as isize).collect();
        self.order.retain(|id| ids.contains(id));
        self.icons.retain(|id, _| ids.contains(id));
        for id in &ids {
            if !self.order.contains(id) {
                self.order.push(*id);
            }
        }
        let fg = unsafe { GetForegroundWindow() }.0 as isize;
        let mut out = Vec::new();
        for id in self.order.iter().take(MAX_APPS) {
            let h = HWND(*id as _);
            let handle = icon_handle(h);
            let key = handle.map_or(0, |i| i.0 as isize);
            let cached = self.icons.get(id).filter(|(k, _)| *k == key).cloned();
            let icon = cached.or_else(|| {
                let px = Arc::new(rasterize(handle?)?);
                self.icons.insert(*id, (key, px.clone()));
                Some((key, px))
            });
            out.push(AppWin { hwnd: *id, title: title(h), active: *id == fg, icon });
        }
        out
    }
}

/// Premier plan, ou réduction si la fenêtre y est déjà (comme un bouton de barre des tâches).
pub fn activate(hwnd: isize) {
    let h = HWND(hwnd as _);
    unsafe {
        if GetForegroundWindow() == h && !IsIconic(h).as_bool() {
            let _ = ShowWindow(h, SW_MINIMIZE);
        } else {
            if IsIconic(h).as_bool() {
                let _ = ShowWindow(h, SW_RESTORE);
            }
            let _ = SetForegroundWindow(h);
        }
    }
}

pub fn minimize(hwnd: isize) {
    unsafe {
        let _ = ShowWindow(HWND(hwnd as _), SW_MINIMIZE);
    }
}

pub fn close(hwnd: isize) {
    unsafe {
        let _ = PostMessageW(HWND(hwnd as _), WM_CLOSE, WPARAM(0), LPARAM(0));
    }
}
