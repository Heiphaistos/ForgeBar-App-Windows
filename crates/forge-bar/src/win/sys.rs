//! Mesures système (CPU, RAM, batterie, son, Wi-Fi) et actions (bureau, volume).
//! Chaque mesure est un appel Windows peu coûteux, fait au rythme du minuteur.

use crate::widgets::{SysData, Wifi};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{FILETIME, HANDLE, HWND};
use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
use windows::Win32::Media::Audio::{eConsole, eRender, IMMDeviceEnumerator, MMDeviceEnumerator};
use windows::Win32::NetworkManagement::WiFi::{
    wlan_intf_opcode_current_connection, WlanCloseHandle, WlanEnumInterfaces, WlanFreeMemory, WlanOpenHandle,
    WlanQueryInterface, WLAN_CONNECTION_ATTRIBUTES, WLAN_INTERFACE_INFO_LIST,
};
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL};
use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
use windows::Win32::System::Threading::GetSystemTimes;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, VIRTUAL_KEY, VK_LWIN,
};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

fn ft(f: FILETIME) -> u64 {
    ((f.dwHighDateTime as u64) << 32) | f.dwLowDateTime as u64
}

#[derive(Default)]
pub struct Sampler {
    prev_idle: u64,
    prev_total: u64,
    audio: Option<IAudioEndpointVolume>,
    wlan: Option<HANDLE>,
    ticks: u64,
    pub data: SysData,
}

impl Drop for Sampler {
    fn drop(&mut self) {
        if let Some(h) = self.wlan.take() {
            unsafe { WlanCloseHandle(h, None) };
        }
    }
}

impl Sampler {
    /// Appelé une fois par seconde, uniquement quand une barre est visible.
    pub fn sample(&mut self) {
        self.ticks += 1;
        self.sample_cpu();
        self.sample_ram();
        self.sample_battery();
        self.sample_volume();
        // Le Wi-Fi change rarement : toutes les 5 secondes suffisent.
        if self.ticks % 5 == 1 {
            self.data.wifi = self.sample_wifi();
        }
        self.data.clock = chrono::Local::now().format("%H:%M").to_string();
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
            self.audio = unsafe {
                let en: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;
                let dev = en.GetDefaultAudioEndpoint(eRender, eConsole).ok()?;
                dev.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None).ok()
            };
        }
        self.audio.as_ref()
    }

    fn sample_volume(&mut self) {
        let read = |ep: &IAudioEndpointVolume| unsafe {
            Some((ep.GetMasterVolumeLevelScalar().ok()?, ep.GetMute().ok()?.as_bool()))
        };
        self.data.volume = self.endpoint().and_then(read);
        if self.data.volume.is_none() {
            // Sortie audio changée ou débranchée : on la redemandera au prochain passage.
            self.audio = None;
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

/// Ouvre une URI ou un fichier avec l'application associée.
pub fn open(target: PCWSTR) {
    unsafe { ShellExecuteW(HWND::default(), w!("open"), target, None, None, SW_SHOWNORMAL) };
}
