//! Notifications audio : Windows prévient quand le volume change et quand la sortie par défaut
//! change, au lieu de l'interroger chaque seconde. Les rappels arrivent sur un fil du système :
//! ils se bornent à poster un message à la fenêtre principale.

use windows::core::{implement, Result, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::Media::Audio::Endpoints::{IAudioEndpointVolumeCallback, IAudioEndpointVolumeCallback_Impl};
use windows::Win32::Media::Audio::{
    EDataFlow, ERole, IMMNotificationClient, IMMNotificationClient_Impl, AUDIO_VOLUME_NOTIFICATION_DATA, DEVICE_STATE,
};
use windows::Win32::UI::Shell::PropertiesSystem::PROPERTYKEY;
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

/// `wParam` du message audio : la sortie par défaut a changé, il faut la redemander.
pub const AUDIO_DEVICE_CHANGED: usize = 1;
/// `wParam` du message audio : le volume a changé ; `lParam` = millièmes de volume | sourdine << 16.
pub const AUDIO_VOLUME_CHANGED: usize = 2;

fn post(target: isize, msg: u32, kind: usize, lparam: isize) {
    unsafe {
        let _ = PostMessageW(HWND(target as _), msg, WPARAM(kind), LPARAM(lparam));
    }
}

#[implement(IMMNotificationClient)]
pub struct DeviceWatcher {
    pub target: isize,
    pub msg: u32,
}

impl IMMNotificationClient_Impl for DeviceWatcher_Impl {
    fn OnDeviceStateChanged(&self, _id: &PCWSTR, _state: DEVICE_STATE) -> Result<()> {
        post(self.target, self.msg, AUDIO_DEVICE_CHANGED, 0);
        Ok(())
    }
    fn OnDeviceAdded(&self, _id: &PCWSTR) -> Result<()> {
        post(self.target, self.msg, AUDIO_DEVICE_CHANGED, 0);
        Ok(())
    }
    fn OnDeviceRemoved(&self, _id: &PCWSTR) -> Result<()> {
        post(self.target, self.msg, AUDIO_DEVICE_CHANGED, 0);
        Ok(())
    }
    fn OnDefaultDeviceChanged(&self, _flow: EDataFlow, _role: ERole, _id: &PCWSTR) -> Result<()> {
        post(self.target, self.msg, AUDIO_DEVICE_CHANGED, 0);
        Ok(())
    }
    fn OnPropertyValueChanged(&self, _id: &PCWSTR, _key: &PROPERTYKEY) -> Result<()> {
        Ok(())
    }
}

#[implement(IAudioEndpointVolumeCallback)]
pub struct VolumeWatcher {
    pub target: isize,
    pub msg: u32,
}

impl IAudioEndpointVolumeCallback_Impl for VolumeWatcher_Impl {
    fn OnNotify(&self, data: *mut AUDIO_VOLUME_NOTIFICATION_DATA) -> Result<()> {
        if let Some(d) = unsafe { data.as_ref() } {
            let milli = (d.fMasterVolume.clamp(0.0, 1.0) * 1000.0).round() as isize;
            post(self.target, self.msg, AUDIO_VOLUME_CHANGED, milli | ((d.bMuted.as_bool() as isize) << 16));
        }
        Ok(())
    }
}

/// Décode le `lParam` d'un message `AUDIO_VOLUME_CHANGED`.
pub fn decode_volume(lparam: isize) -> (f32, bool) {
    ((lparam & 0xFFFF) as f32 / 1000.0, lparam >> 16 != 0)
}
