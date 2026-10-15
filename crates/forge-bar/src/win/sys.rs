//! Mesures système (CPU, RAM, batterie, son, Wi-Fi) et actions (bureau, volume).
//! Chaque mesure est un appel Windows peu coûteux, fait au rythme du minuteur.

use super::apps::Windows;
use super::audio::{self, DeviceWatcher, VolumeWatcher};
use super::metrics::{Gpu, Net};
use super::thermal::Thermal;
use crate::widgets::{SysData, Wifi};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{FILETIME, HANDLE, HWND};
use windows::Win32::Media::Audio::Endpoints::{IAudioEndpointVolume, IAudioEndpointVolumeCallback};
use windows::Win32::Media::Audio::{eConsole, eRender, IMMDeviceEnumerator, IMMNotificationClient, MMDeviceEnumerator};
use windows::Win32::NetworkManagement::WiFi::{
    wlan_intf_opcode_current_connection, WlanCloseHandle, WlanEnumInterfaces, WlanFreeMemory, WlanOpenHandle,
    WlanQueryInterface, WLAN_CONNECTION_ATTRIBUTES, WLAN_INTERFACE_INFO_LIST,
};
use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL};
use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
use windows::Win32::System::Threading::GetSystemTimes;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, VIRTUAL_KEY, VK_LWIN,
};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{SW_HIDE, SW_SHOWNORMAL};

fn ft(f: FILETIME) -> u64 {
    ((f.dwHighDateTime as u64) << 32) | f.dwLowDateTime as u64
}

/// Sortie audio suivie : le volume est poussé par Windows via le rappel, jamais interrogé en boucle.
struct Endpoint {
    volume: IAudioEndpointVolume,
    callback: IAudioEndpointVolumeCallback,
}

impl Drop for Endpoint {
    fn drop(&mut self) {
        unsafe {
            let _ = self.volume.UnregisterControlChangeNotify(&self.callback);
        }
    }
}

#[derive(Default)]
pub struct Sampler {
    prev_idle: u64,
    prev_total: u64,
    audio: Option<Endpoint>,
    enumerator: Option<(IMMDeviceEnumerator, IMMNotificationClient)>,
    /// Fenêtre principale et message qui reçoivent les notifications audio.
    target: (isize, u32),
    wlan: Option<HANDLE>,
    gpu: Gpu,
    net: Net,
    /// Widgets coûteux : mesurés seulement s'ils sont dans une disposition.
    pub want_gpu: bool,
    pub want_net: bool,
    pub want_disk: bool,
    pub want_apps: bool,
    pub want_temp: bool,
    /// Mode jeu : une application plein écran est au premier plan, on ne touche plus aux autres fenêtres.
    pub game_mode: bool,
    thermal: Thermal,
    windows: Windows,
    ticks: u64,
    pub data: SysData,
}

impl Drop for Sampler {
    fn drop(&mut self) {
        self.audio = None;
        if let Some((en, client)) = self.enumerator.take() {
            unsafe {
                let _ = en.UnregisterEndpointNotificationCallback(&client);
            }
        }
        if let Some(h) = self.wlan.take() {
            unsafe { WlanCloseHandle(h, None) };
        }
    }
}

impl Sampler {
    /// Branche les notifications audio sur la fenêtre principale.
    pub fn watch_audio(&mut self, main: isize, msg: u32) {
        self.target = (main, msg);
        unsafe {
            let Ok(en) = CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL) else {
                return;
            };
            let client: IMMNotificationClient = DeviceWatcher { target: main, msg }.into();
            if en.RegisterEndpointNotificationCallback(&client).is_ok() {
                self.enumerator = Some((en, client));
            }
        }
        self.sample_volume();
    }

    /// Message audio reçu de Windows (voir `audio::AUDIO_*`).
    pub fn on_audio_message(&mut self, kind: usize, lparam: isize) {
        match kind {
            audio::AUDIO_VOLUME_CHANGED => self.data.volume = Some(audio::decode_volume(lparam)),
            _ => {
                self.audio = None; // sortie changée : on se rattache à la nouvelle
                self.sample_volume();
            }
        }
    }

    /// Appelé une fois par seconde, uniquement quand une barre est visible.
    pub fn sample(&mut self) {
        self.ticks += 1;
        self.sample_cpu();
        self.sample_ram();
        self.sample_battery();
        if self.data.volume.is_none() && self.ticks.is_multiple_of(10) {
            self.sample_volume(); // pas de sortie audio au démarrage : on réessaie de temps en temps
        }
        // Le Wi-Fi change rarement : toutes les 5 secondes suffisent.
        if self.ticks % 5 == 1 {
            self.data.wifi = self.sample_wifi();
        }
        if self.want_gpu && self.ticks.is_multiple_of(2) {
            self.data.gpu = self.gpu.sample();
        }
        if self.want_net {
            self.data.net = self.net.sample();
        }
        // L'occupation d'un disque bouge lentement : toutes les 10 secondes.
        if self.want_disk && self.ticks % 10 == 1 {
            self.data.disk = system_drive_usage();
        }
        self.game_mode = foreground_is_fullscreen();
        self.data.apps = if self.want_apps && !self.game_mode { self.windows.list() } else { Vec::new() };
        if self.want_temp && self.ticks % 3 == 1 {
            (self.data.cpu_temp, self.data.gpu_temp) = self.thermal.sample();
        }
        match super::fps::state() {
            super::fps::STATE_RUNNING => self.data.fps = super::fps::sample(foreground_pid()),
            super::fps::STATE_DENIED => self.data.fps_denied = true,
            _ => {}
        }
        self.data.clock = chrono::Local::now().format("%H:%M").to_string();
        let snapshot = self.data.clone();
        self.data.hist.record(&snapshot);
    }

    fn sample_cpu(&mut self) {
        let (mut idle, mut kernel, mut user) = (FILETIME::default(), FILETIME::default(), FILETIME::default());
        if unsafe { GetSystemTimes(Some(&mut idle), Some(&mut kernel), Some(&mut user)) }.is_err() {
            return;
        }
        let (idle, total) = (ft(idle), ft(kernel) + ft(user));
        let (di, dt) = (idle.saturating_sub(self.prev_idle), total.saturating_sub(self.prev_total));
        if self.prev_total != 0 && dt > 0 {
            self.data.cpu = (100.0 * (1.0 - di as f64 / dt as f64)).clamp(0.0, 100.0) as f32;
        }
        self.prev_idle = idle;
        self.prev_total = total;
    }

    fn sample_ram(&mut self) {
        let mut m = MEMORYSTATUSEX { dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32, ..Default::default() };
        if unsafe { GlobalMemoryStatusEx(&mut m) }.is_ok() {
            self.data.ram = m.dwMemoryLoad as f32;
        }
    }

    fn sample_battery(&mut self) {
        let mut s = SYSTEM_POWER_STATUS::default();
        self.data.battery = None;
        if unsafe { GetSystemPowerStatus(&mut s) }.is_ok() && s.BatteryFlag & 128 == 0 && s.BatteryLifePercent <= 100 {
            self.data.battery = Some((s.BatteryLifePercent, s.ACLineStatus == 1));
        }
    }

    fn endpoint(&mut self) -> Option<&IAudioEndpointVolume> {
        if self.audio.is_none() {
            let (main, msg) = self.target;
            self.audio = unsafe {
                let en: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;
                let dev = en.GetDefaultAudioEndpoint(eRender, eConsole).ok()?;
                let volume = dev.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None).ok()?;
                let callback: IAudioEndpointVolumeCallback = VolumeWatcher { target: main, msg }.into();
                if main != 0 {
                    volume.RegisterControlChangeNotify(&callback).ok()?;
                }
                Some(Endpoint { volume, callback })
            };
        }
        self.audio.as_ref().map(|e| &e.volume)
    }

    fn sample_volume(&mut self) {
        let read = |ep: &IAudioEndpointVolume| unsafe {
            Some((ep.GetMasterVolumeLevelScalar().ok()?, ep.GetMute().ok()?.as_bool()))
        };
        self.data.volume = self.endpoint().and_then(read);
        if self.data.volume.is_none() {
            self.audio = None; // aucune sortie : on réessaiera
        }
    }

    pub fn change_volume(&mut self, delta: f32) {
        if let Some(ep) = self.endpoint() {
            unsafe {
                if let Ok(v) = ep.GetMasterVolumeLevelScalar() {
                    let _ = ep.SetMasterVolumeLevelScalar((v + delta).clamp(0.0, 1.0), std::ptr::null());
                }
            }
        }
        self.sample_volume();
    }

    /// Fixe le volume (0 à 1) et sort de la sourdine si on monte le son.
    pub fn set_volume(&mut self, v: f32) {
        if let Some(ep) = self.endpoint() {
            unsafe {
                let _ = ep.SetMasterVolumeLevelScalar(v.clamp(0.0, 1.0), std::ptr::null());
                if v > 0.0 {
                    let _ = ep.SetMute(false, std::ptr::null());
                }
            }
        }
        self.sample_volume();
    }

    pub fn toggle_mute(&mut self) {
        if let Some(ep) = self.endpoint() {
            unsafe {
                if let Ok(m) = ep.GetMute() {
                    let _ = ep.SetMute(!m.as_bool(), std::ptr::null());
                }
            }
        }
        self.sample_volume();
    }

    fn sample_wifi(&mut self) -> Option<Wifi> {
        unsafe {
            if self.wlan.is_none() {
                let (mut ver, mut h) = (0u32, HANDLE::default());
                if WlanOpenHandle(2, None, &mut ver, &mut h) != 0 {
                    return None;
                }
                self.wlan = Some(h);
            }
            let h = self.wlan?;
            let mut list: *mut WLAN_INTERFACE_INFO_LIST = std::ptr::null_mut();
            if WlanEnumInterfaces(h, None, &mut list) != 0 || list.is_null() {
                return None;
            }
            let count = (*list).dwNumberOfItems as usize;
            let infos = std::slice::from_raw_parts((*list).InterfaceInfo.as_ptr(), count);
            let mut result = if count > 0 { Some(Wifi::default()) } else { None };
            for info in infos {
                let (mut size, mut data) = (0u32, std::ptr::null_mut());
                let rc = WlanQueryInterface(
                    h,
                    &info.InterfaceGuid,
                    wlan_intf_opcode_current_connection,
                    None,
                    &mut size,
                    &mut data,
                    None,
                );
                if rc == 0 && !data.is_null() {
                    let attr = &*(data as *const WLAN_CONNECTION_ATTRIBUTES);
                    let ssid = &attr.wlanAssociationAttributes.dot11Ssid;
                    let len = (ssid.uSSIDLength as usize).min(32);
                    result = Some(Wifi {
                        ssid: String::from_utf8_lossy(&ssid.ucSSID[..len]).to_string(),
                        signal: attr.wlanAssociationAttributes.wlanSignalQuality,
                    });
                    WlanFreeMemory(data);
                    break;
                }
            }
            WlanFreeMemory(list as _);
            result
        }
    }
}

/// Part utilisée du lecteur système, en pourcentage.
fn system_drive_usage() -> Option<f32> {
    let root = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into()) + "\\";
    let wide: Vec<u16> = root.encode_utf16().chain([0]).collect();
    let (mut total, mut free) = (0u64, 0u64);
    unsafe { GetDiskFreeSpaceExW(PCWSTR(wide.as_ptr()), None, Some(&mut total), Some(&mut free)) }.ok()?;
    (total > 0).then(|| 100.0 * (1.0 - free as f64 / total as f64) as f32)
}

/// Vrai si la fenêtre au premier plan couvre tout son écran sans cadre (jeu ou vidéo en plein écran).
/// Lecture de géométrie seulement : aucun message n'est envoyé à cette fenêtre.
fn foreground_is_fullscreen() -> bool {
    use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST};
    use windows::Win32::UI::WindowsAndMessaging::{
        GetClassNameW, GetForegroundWindow, GetWindowLongW, GetWindowRect, GWL_STYLE, WS_CAPTION,
    };
    unsafe {
        let fg = GetForegroundWindow();
        if fg.is_invalid() {
            return false;
        }
        let mut class = [0u16; 32];
        let n = GetClassNameW(fg, &mut class) as usize;
        if matches!(String::from_utf16_lossy(&class[..n]).as_str(), "Progman" | "WorkerW" | "Shell_TrayWnd") {
            return false;
        }
        let mut r = windows::Win32::Foundation::RECT::default();
        let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        if GetWindowRect(fg, &mut r).is_err()
            || !GetMonitorInfoW(MonitorFromWindow(fg, MONITOR_DEFAULTTONEAREST), &mut mi).as_bool()
        {
            return false;
        }
        let m = mi.rcMonitor;
        let covers = r.left <= m.left && r.top <= m.top && r.right >= m.right && r.bottom >= m.bottom;
        covers && GetWindowLongW(fg, GWL_STYLE) as u32 & WS_CAPTION.0 != WS_CAPTION.0
    }
}

/// Processus de la fenêtre au premier plan (lecture de l'identifiant seulement : aucun handle sur le processus).
fn foreground_pid() -> u32 {
    let mut pid = 0u32;
    unsafe {
        windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId(
            windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow(),
            Some(&mut pid),
        )
    };
    pid
}

fn key(vk: VIRTUAL_KEY, up: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

/// Affiche ou restaure le bureau, comme Win + D.
pub fn toggle_desktop() {
    let d = VIRTUAL_KEY(0x44);
    let inputs = [key(VK_LWIN, false), key(d, false), key(d, true), key(VK_LWIN, true)];
    unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
}

/// Touche multimédia : 0xB3 lecture / pause, 0xB0 piste suivante, 0xB1 piste précédente.
pub fn media_key(code: u16) {
    let k = VIRTUAL_KEY(code);
    let inputs = [key(k, false), key(k, true)];
    unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
}

/// Ajoute l'utilisateur au groupe « Utilisateurs du journal de performances » (SID S-1-5-32-559), seul droit
/// nécessaire au suivi des FPS. Demande l'élévation (UAC) une fois ; effectif à la prochaine ouverture de session.
pub fn allow_fps() {
    let user = format!(
        "{}\\{}",
        std::env::var("USERDOMAIN").unwrap_or_default(),
        std::env::var("USERNAME").unwrap_or_default()
    )
    .replace('\'', "''");
    let args = format!(
        "-NoProfile -WindowStyle Hidden -Command \"Add-LocalGroupMember -Group (Get-LocalGroup -SID S-1-5-32-559) -Member '{user}'\""
    );
    let wide: Vec<u16> = args.encode_utf16().chain([0]).collect();
    unsafe {
        ShellExecuteW(HWND::default(), w!("runas"), w!("powershell.exe"), PCWSTR(wide.as_ptr()), None, SW_HIDE);
    }
}

/// Ouvre une URI ou un fichier avec l'application associée.
pub fn open(target: PCWSTR) {
    unsafe { ShellExecuteW(HWND::default(), w!("open"), target, None, None, SW_SHOWNORMAL) };
}
