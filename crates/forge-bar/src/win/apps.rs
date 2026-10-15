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
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::UI::WindowsAndMessaging::*;

pub const SIZE: i32 = 32;
const SIZE_PX: i32 = SIZE;
/// Au-delà, les miniatures prendraient toute la barre.
pub const MAX_APPS: usize = 16;

#[derive(Default)]
pub struct Windows {
    /// Ordre d'apparition : une fenêtre garde sa place tant qu'elle existe.
    order: Vec<isize>,
    icons: HashMap<isize, (isize, Arc<Vec<u8>>)>,
    /// Compteur d'appels : l'icône d'une fenêtre est relue toutes les 60 listes seulement.
    age: u32,
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
        // Mêmes règles que la barre des tâches : ni fenêtre outil, ni fenêtre possédée, ni fenêtre qui ne prend
        // jamais le focus (surcouches), sauf si l'application demande explicitement un bouton (WS_EX_APPWINDOW).
        let hidden_kind = ex & WS_EX_TOOLWINDOW.0 != 0 || ex & WS_EX_NOACTIVATE.0 != 0 || owned;
        if hidden_kind && ex & WS_EX_APPWINDOW.0 == 0 {
            return false;
        }
        // Pas nos propres fenêtres (barre, panneaux).
        let mut pid = 0u32;
        GetWindowThreadProcessId(h, Some(&mut pid));
        if pid == GetCurrentProcessId() {
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
        self.age = self.age.wrapping_add(1);
        let refresh = self.age.is_multiple_of(60);
        let mut out = Vec::new();
        for id in self.order.iter().take(MAX_APPS) {
            let h = HWND(*id as _);
            // Une icône déjà trouvée est gardée : on ne renvoie pas un message à chaque fenêtre chaque seconde.
            let icon = match self.icons.get(id).cloned() {
                Some(c) if !refresh => Some(c),
                _ => {
                    let found =
                        icon_handle(h).and_then(|i| Some((i.0 as isize, Arc::new(rasterize(i)?)))).or_else(|| {
                            // Applications UWP (Paramètres, Calculatrice…) : pas d'icône par message, mais le shell en a une.
                            shell_icon(h).map(|px| (-1, Arc::new(px)))
                        });
                    if let Some(f) = &found {
                        self.icons.insert(*id, f.clone());
                    }
                    found
                }
            };
            out.push(AppWin { hwnd: *id, title: title(h), active: *id == fg, icon });
        }
        out
    }
}

/// Icône du shell pour une fenêtre UWP, retrouvée par son identifiant d'application (AppUserModelID).
fn shell_icon(h: HWND) -> Option<Vec<u8>> {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::SIZE;
    use windows::Win32::Graphics::Gdi::{GetDC, GetDIBits, GetObjectW, ReleaseDC, BITMAP, DIB_RGB_COLORS};
    use windows::Win32::System::Com::StructuredStorage::PropVariantToStringAlloc;
    use windows::Win32::UI::Shell::PropertiesSystem::PROPERTYKEY;
    use windows::Win32::UI::Shell::PropertiesSystem::{IPropertyStore, SHGetPropertyStoreForWindow};
    use windows::Win32::UI::Shell::{
        IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_BIGGERSIZEOK, SIIGBF_ICONONLY,
    };
    unsafe {
        let store: IPropertyStore = SHGetPropertyStoreForWindow(h).ok()?;
        // System.AppUserModel.ID {9F4C2855-9F79-4B39-A8D0-E1D42DE1D5F3}, 5
        let key = PROPERTYKEY { fmtid: windows::core::GUID::from_u128(0x9F4C2855_9F79_4B39_A8D0_E1D42DE1D5F3), pid: 5 };
        let value = store.GetValue(&key).ok()?;
        let id = PropVariantToStringAlloc(&value).ok()?.to_string().ok()?;
        if id.is_empty() {
            return None;
        }
        let path: Vec<u16> = format!("shell:AppsFolder\\{id}").encode_utf16().chain([0]).collect();
        let factory: IShellItemImageFactory = SHCreateItemFromParsingName(PCWSTR(path.as_ptr()), None).ok()?;
        let bmp = factory.GetImage(SIZE { cx: SIZE_PX, cy: SIZE_PX }, SIIGBF_ICONONLY | SIIGBF_BIGGERSIZEOK).ok()?;
        let mut info = BITMAP::default();
        GetObjectW(bmp, std::mem::size_of::<BITMAP>() as i32, Some(&mut info as *mut _ as _));
        let (w, hgt) = (info.bmWidth, info.bmHeight);
        let mut px = vec![0u8; (w * hgt * 4) as usize];
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -hgt,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let dc = GetDC(None);
        let rows =
            GetDIBits(dc, bmp, 0, hgt as u32, Some(px.as_mut_ptr() as _), &bmi as *const _ as *mut _, DIB_RGB_COLORS);
        ReleaseDC(None, dc);
        let _ = DeleteObject(bmp);
        if rows == 0 || w != SIZE_PX || hgt != SIZE_PX {
            return None;
        }
        // Le shell rend de l'alpha « droit » : Direct2D attend des couleurs prémultipliées.
        for p in px.chunks_exact_mut(4) {
            let a = p[3] as u32;
            for c in &mut p[..3] {
                *c = (*c as u32 * a / 255) as u8;
            }
        }
        Some(px)
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
            // Une fenêtre élevée (Gestionnaire des tâches…) ignore SetForegroundWindow et ShowWindow venant d'un
            // processus non élevé : SwitchToThisWindow, que le shell utilise aussi, passe.
            SwitchToThisWindow(h, true);
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
