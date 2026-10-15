//! Images par seconde sans rien injecter dans les jeux : une session ETW (suivi d'événements Windows) écoute les
//! événements « Present » de DXGI et Direct3D 9, comme PresentMon. Aucun hook, aucune DLL dans le jeu, aucun
//! pilote, aucun handle ouvert sur un autre processus : c'est ce qui la rend compatible avec les anti-triche.
//! Windows réserve les sessions ETW aux administrateurs et au groupe « Utilisateurs du journal de performances ».

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Mutex;
use windows::core::{GUID, PWSTR};
use windows::Win32::Foundation::ERROR_ALREADY_EXISTS;
use windows::Win32::System::Diagnostics::Etw::{
    CloseTrace, ControlTraceW, EnableTraceEx2, OpenTraceW, ProcessTrace, StartTraceW, CONTROLTRACE_HANDLE,
    EVENT_CONTROL_CODE_ENABLE_PROVIDER, EVENT_RECORD, EVENT_TRACE_CONTROL_STOP, EVENT_TRACE_LOGFILEW,
    EVENT_TRACE_PROPERTIES, EVENT_TRACE_REAL_TIME_MODE, PROCESS_TRACE_MODE_EVENT_RECORD, PROCESS_TRACE_MODE_REAL_TIME,
    TRACE_LEVEL_INFORMATION, WNODE_FLAG_TRACED_GUID,
};

const SESSION: &str = "ForgeBarFps";
/// Microsoft-Windows-DXGI, événement 42 (Present::Start).
const DXGI: GUID = GUID::from_u128(0xCA11C036_0102_4A2D_A6AD_F03CFED5D3C9);
/// Microsoft-Windows-D3D9, événement 1 (Present).
const D3D9: GUID = GUID::from_u128(0x783ACA0A_790E_4D7F_8451_AA850511C6B9);
/// Accès refusé : ni administrateur ni membre du groupe « Utilisateurs du journal de performances ».
const ACCESS_DENIED: u32 = 5;

pub const STATE_IDLE: u8 = 0;
pub const STATE_RUNNING: u8 = 1;
pub const STATE_DENIED: u8 = 2;
pub const STATE_FAILED: u8 = 3;
static STATE: AtomicU8 = AtomicU8::new(STATE_IDLE);

/// (processus, chaîne d'échange, présentations) depuis le dernier relevé. Une chaîne d'échange = une fenêtre de rendu :
/// un jeu avec surcouche (Steam, Discord, NVIDIA…) en a deux, et seule celle du jeu doit compter.
static COUNTS: Mutex<Vec<(u32, u64, u32)>> = Mutex::new(Vec::new());

/// `DXGI_PRESENT_TEST` : appel qui ne présente rien (test d'occultation), fait par beaucoup de jeux à chaque image.
const DXGI_PRESENT_TEST: u32 = 0x1;
/// `EVENT_HEADER_FLAG_32_BIT_HEADER` : processus 32 bits, pointeurs de 4 octets dans les données de l'événement.
const HEADER_32_BIT: u16 = 0x20;

pub fn state() -> u8 {
    STATE.load(Ordering::Relaxed)
}

unsafe extern "system" fn on_event(rec: *mut EVENT_RECORD) {
    let h = &(*rec).EventHeader;
    let id = h.EventDescriptor.Id;
    if !((h.ProviderId == DXGI && id == 42) || (h.ProviderId == D3D9 && id == 1)) {
        return;
    }
    // Données de l'événement : pointeur de la chaîne d'échange, puis les indicateurs de l'appel (comme PresentMon).
    let ptr = if h.Flags & HEADER_32_BIT != 0 { 4 } else { 8 };
    let len = (*rec).UserDataLength as usize;
    if (*rec).UserData.is_null() || len < ptr + 4 {
        return;
    }
    let data = std::slice::from_raw_parts((*rec).UserData as *const u8, len);
    let mut chain = [0u8; 8];
    chain[..ptr].copy_from_slice(&data[..ptr]);
    let chain = u64::from_le_bytes(chain);
    if h.ProviderId == DXGI {
        let flags = u32::from_le_bytes([data[ptr], data[ptr + 1], data[ptr + 2], data[ptr + 3]]);
        if flags & DXGI_PRESENT_TEST != 0 {
            return;
        }
    }
    if let Ok(mut v) = COUNTS.lock() {
        match v.iter_mut().find(|(p, c, _)| *p == h.ProcessId && *c == chain) {
            Some((_, _, n)) => *n += 1,
            None => v.push((h.ProcessId, chain, 1)),
        }
    }
}

fn session_name() -> Vec<u16> {
    SESSION.encode_utf16().chain([0]).collect()
}

/// Tampon `EVENT_TRACE_PROPERTIES` suivi du nom de session, comme l'exige l'API.
fn properties() -> Vec<u8> {
    let head = std::mem::size_of::<EVENT_TRACE_PROPERTIES>();
    let size = head + session_name().len() * 2;
    let mut buf = vec![0u8; size];
    let p = buf.as_mut_ptr() as *mut EVENT_TRACE_PROPERTIES;
    unsafe {
        (*p).Wnode.BufferSize = size as u32;
        (*p).Wnode.Flags = WNODE_FLAG_TRACED_GUID;
        (*p).Wnode.ClientContext = 1;
        (*p).LogFileMode = EVENT_TRACE_REAL_TIME_MODE;
        (*p).LoggerNameOffset = head as u32;
    }
    buf
}

fn stop_session() {
    let mut buf = properties();
    unsafe {
        let name = session_name();
        let _ = ControlTraceW(
            CONTROLTRACE_HANDLE::default(),
            windows::core::PCWSTR(name.as_ptr()),
            buf.as_mut_ptr() as *mut EVENT_TRACE_PROPERTIES,
            EVENT_TRACE_CONTROL_STOP,
        );
    }
}

fn run() -> Result<(), u32> {
    let name = session_name();
    let mut handle = CONTROLTRACE_HANDLE::default();
    let mut buf = properties();
    let mut rc = unsafe { StartTraceW(&mut handle, windows::core::PCWSTR(name.as_ptr()), buf.as_mut_ptr() as *mut _) };
    if rc == ERROR_ALREADY_EXISTS {
        // Session restée d'un arrêt brutal (mise à jour, plantage) : on la ferme et on recommence.
        stop_session();
        buf = properties();
        rc = unsafe { StartTraceW(&mut handle, windows::core::PCWSTR(name.as_ptr()), buf.as_mut_ptr() as *mut _) };
    }
    if rc.0 != 0 {
        return Err(rc.0);
    }
    for guid in [DXGI, D3D9] {
        let rc = unsafe {
            EnableTraceEx2(
                handle,
                &guid,
                EVENT_CONTROL_CODE_ENABLE_PROVIDER.0,
                TRACE_LEVEL_INFORMATION as u8,
                u64::MAX,
                0,
                0,
                None,
            )
        };
        if rc.0 != 0 {
            stop_session();
            return Err(rc.0);
        }
    }
    let mut wname = session_name();
    let mut log = EVENT_TRACE_LOGFILEW { LoggerName: PWSTR(wname.as_mut_ptr()), ..Default::default() };
    log.Anonymous1.ProcessTraceMode = PROCESS_TRACE_MODE_REAL_TIME | PROCESS_TRACE_MODE_EVENT_RECORD;
    log.Anonymous2.EventRecordCallback = Some(on_event);
    let trace = unsafe { OpenTraceW(&mut log) };
    if trace.Value == u64::MAX {
        stop_session();
        return Err(0xFFFF);
    }
    STATE.store(STATE_RUNNING, Ordering::Relaxed);
    // Bloque jusqu'à l'arrêt de la session (`stop`).
    unsafe {
        let _ = ProcessTrace(&[trace], None, None);
        let _ = CloseTrace(trace);
    }
    Ok(())
}

/// Démarre l'écoute sur un fil de fond (une seule fois).
pub fn start() {
    if STATE.compare_exchange(STATE_IDLE, STATE_FAILED, Ordering::Relaxed, Ordering::Relaxed).is_err() {
        return;
    }
    std::thread::spawn(|| {
        let result = run();
        STATE.store(
            match result {
                Err(ACCESS_DENIED) => STATE_DENIED,
                Err(_) => STATE_FAILED,
                Ok(()) => STATE_IDLE,
            },
            Ordering::Relaxed,
        );
    });
}

pub fn stop() {
    if state() == STATE_RUNNING {
        stop_session();
    }
}

/// Images par seconde de l'application au premier plan (`fg_pid`) ; `None` si elle ne présente presque rien.
/// Pas de repli sur « le processus le plus actif » : ce serait le compositeur Windows (dwm.exe, ~60 images).
/// Appelé une fois par seconde : le compteur est remis à zéro.
pub fn sample(fg_pid: u32) -> Option<u32> {
    let counts = std::mem::take(&mut *COUNTS.lock().ok()?);
    // Plusieurs chaînes dans le même processus (jeu + surcouche) : la plus active est celle du jeu.
    counts.iter().filter(|(p, _, _)| *p == fg_pid).map(|(_, _, n)| *n).max().filter(|n| *n >= 5)
}

#[cfg(test)]
mod tests {
    /// Session ETW réelle (administrateur ou groupe Journal de performances) : `cargo test -- --ignored live_fps`.
    #[test]
    #[ignore]
    fn live_fps() {
        super::start();
        std::thread::sleep(std::time::Duration::from_secs(4));
        assert_eq!(super::state(), super::STATE_RUNNING, "session non démarrée (état {})", super::state());
        let counts = super::COUNTS.lock().unwrap().clone();
        println!("présentations par processus en 4 s : {counts:?}");
        super::stop();
        assert!(!counts.is_empty(), "aucun événement Present reçu");
    }
}
