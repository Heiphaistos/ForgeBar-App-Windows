//! Températures sans pilote noyau : GPU NVIDIA via NVML (bibliothèque livrée avec le pilote graphique,
//! la même que `nvidia-smi`) et zones thermiques ACPI via les compteurs de performance.
//! Aucun pilote tiers de type WinRing0 : ils sont bloqués par les anti-triche, on n'en charge jamais.

use std::ffi::c_void;
use windows::core::{s, w, PCWSTR};
use windows::Win32::Foundation::HMODULE;
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryExW, LOAD_LIBRARY_SEARCH_SYSTEM32};
use windows::Win32::System::Performance::{
    PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterArrayW, PdhOpenQueryW,
    PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_DOUBLE,
};

type NvmlInit = unsafe extern "C" fn() -> i32;
type NvmlHandle = unsafe extern "C" fn(u32, *mut *mut c_void) -> i32;
type NvmlTemp = unsafe extern "C" fn(*mut c_void, u32, *mut u32) -> i32;

/// Première carte NVIDIA, fonctions NVML résolues une fois.
struct Nvml {
    device: *mut c_void,
    temp: NvmlTemp,
}

fn load_nvml() -> Option<Nvml> {
    unsafe {
        // Dossier système uniquement : pas de recherche dans le dossier courant (détournement de DLL).
        let lib: HMODULE = LoadLibraryExW(w!("nvml.dll"), None, LOAD_LIBRARY_SEARCH_SYSTEM32).ok()?;
        let init: NvmlInit = std::mem::transmute(GetProcAddress(lib, s!("nvmlInit_v2"))?);
        let handle: NvmlHandle = std::mem::transmute(GetProcAddress(lib, s!("nvmlDeviceGetHandleByIndex_v2"))?);
        let temp: NvmlTemp = std::mem::transmute(GetProcAddress(lib, s!("nvmlDeviceGetTemperature"))?);
        if init() != 0 {
            return None;
        }
        let mut device = std::ptr::null_mut();
        (handle(0, &mut device) == 0).then_some(Nvml { device, temp })
    }
}

#[derive(Default)]
pub struct Thermal {
    nvml: Option<Nvml>,
    nvml_tried: bool,
    pdh: Option<(isize, isize)>,
    pdh_tried: bool,
}

impl Drop for Thermal {
    fn drop(&mut self) {
        if let Some((q, _)) = self.pdh.take() {
            unsafe { PdhCloseQuery(q) };
        }
    }
}

impl Thermal {
    /// Température du GPU NVIDIA en °C.
    fn gpu(&mut self) -> Option<f32> {
        if !self.nvml_tried {
            self.nvml_tried = true;
            self.nvml = load_nvml();
        }
        let n = self.nvml.as_ref()?;
        let mut t = 0u32;
        (unsafe { (n.temp)(n.device, 0, &mut t) } == 0).then_some(t as f32)
    }

    /// Température la plus haute des zones thermiques ACPI, si la machine en expose.
    fn zones(&mut self) -> Option<f32> {
        if !self.pdh_tried {
            self.pdh_tried = true;
            unsafe {
                let (mut q, mut c) = (0isize, 0isize);
                let ok = PdhOpenQueryW(PCWSTR::null(), 0, &mut q) == 0
                    && PdhAddEnglishCounterW(q, w!("\\Thermal Zone Information(*)\\Temperature"), 0, &mut c) == 0;
                if ok {
                    self.pdh = Some((q, c));
                } else if q != 0 {
                    PdhCloseQuery(q);
                }
            }
        }
        let (q, c) = self.pdh?;
        unsafe {
            if PdhCollectQueryData(q) != 0 {
                return None;
            }
            let (mut size, mut count) = (0u32, 0u32);
            PdhGetFormattedCounterArrayW(c, PDH_FMT_DOUBLE, &mut size, &mut count, None);
            if size == 0 {
                return None;
            }
            let mut buf = vec![0u8; size as usize];
            let items = buf.as_mut_ptr() as *mut PDH_FMT_COUNTERVALUE_ITEM_W;
            if PdhGetFormattedCounterArrayW(c, PDH_FMT_DOUBLE, &mut size, &mut count, Some(items)) != 0 {
                return None;
            }
            std::slice::from_raw_parts(items, count as usize)
                .iter()
                .map(|it| it.FmtValue.Anonymous.doubleValue - 273.15)
                .filter(|t| (10.0..120.0).contains(t))
                .fold(None, |m: Option<f64>, t| Some(m.map_or(t, |x| x.max(t))))
                .map(|t| t as f32)
        }
    }

    /// (processeur ou zone ACPI, GPU) en °C ; `None` quand la machine n'expose rien.
    pub fn sample(&mut self) -> (Option<f32>, Option<f32>) {
        (self.zones(), self.gpu())
    }
}
