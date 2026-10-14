//! Média en cours (Spotify, navigateur, lecteur vidéo…) via GlobalSystemMediaTransportControls.
//! Un fil de fond interroge le gestionnaire toutes les 2 secondes et ne réveille la barre
//! que si le titre ou l'état de lecture ont changé.

use crate::widgets::Media;
use std::sync::{Arc, Mutex};
use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSessionManager, GlobalSystemMediaTransportControlsSessionPlaybackStatus,
};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

fn read(manager: &GlobalSystemMediaTransportControlsSessionManager) -> windows::core::Result<Option<Media>> {
    let Ok(session) = manager.GetCurrentSession() else { return Ok(None) };
    let props = session.TryGetMediaPropertiesAsync()?.get()?;
    let status = session.GetPlaybackInfo()?.PlaybackStatus()?;
    let title = props.Title()?.to_string();
    if title.is_empty() {
        return Ok(None);
    }
    Ok(Some(Media {
        title,
        artist: props.Artist()?.to_string(),
        playing: status == GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing,
    }))
}

pub fn spawn(main: HWND, msg: u32, out: Arc<Mutex<Option<Media>>>) {
    let main = main.0 as isize;
    std::thread::spawn(move || {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        let mut manager = None;
        loop {
            if manager.is_none() {
                manager = GlobalSystemMediaTransportControlsSessionManager::RequestAsync().and_then(|a| a.get()).ok();
            }
            let now = manager.as_ref().and_then(|m| read(m).ok()).flatten();
            let changed = {
                let mut cur = out.lock().unwrap();
                let changed = *cur != now;
                *cur = now;
                changed
            };
            if changed {
                unsafe {
                    let _ = PostMessageW(HWND(main as _), msg, WPARAM(0), LPARAM(0));
                }
            }
            std::thread::sleep(std::time::Duration::from_secs(2));
        }
    });
}
