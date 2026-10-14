//! Mesures GPU (compteurs PDH « GPU Engine ») et débit réseau (GetIfTable2).

use std::collections::HashMap;
use windows::core::{w, PCWSTR};
use windows::Win32::NetworkManagement::IpHelper::{FreeMibTable, GetIfTable2, MIB_IF_TABLE2};
use windows::Win32::System::Performance::{
    PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterArrayW, PdhOpenQueryW,
    PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_DOUBLE,
};

const IF_TYPE_SOFTWARE_LOOPBACK: u32 = 24;
const IF_OPER_STATUS_UP: i32 = 1;

/// Charge GPU : somme des moteurs par type (3D, Copy, Video…), le plus chargé l'emporte,
/// comme le Gestionnaire des tâches. `None` si les compteurs n'existent pas (pilote ancien, VM).
#[derive(Default)]
pub struct Gpu {
    query: isize,
    counter: isize,
    failed: bool,
    primed: bool,
}

impl Drop for Gpu {
    fn drop(&mut self) {
        if self.query != 0 {
            unsafe { PdhCloseQuery(self.query) };
        }
    }
}

impl Gpu {
    fn open(&mut self) -> bool {
        if self.query != 0 {
            return true;
        }
        if self.failed {
            return false;
        }
        unsafe {
            let (mut q, mut c) = (0isize, 0isize);
            let ok = PdhOpenQueryW(PCWSTR::null(), 0, &mut q) == 0
                && PdhAddEnglishCounterW(q, w!("\\GPU Engine(*)\\Utilization Percentage"), 0, &mut c) == 0;
            if !ok {
                if q != 0 {
                    PdhCloseQuery(q);
                }
                self.failed = true;
                return false;
            }
            self.query = q;
            self.counter = c;
        }
        true
    }

    pub fn sample(&mut self) -> Option<f32> {
        if !self.open() {
            return None;
        }
        unsafe {
            if PdhCollectQueryData(self.query) != 0 {
                return None;
            }
            if !self.primed {
                // Un compteur de taux a besoin de deux relevés pour donner une valeur.
                self.primed = true;
                return None;
            }
            let (mut size, mut count) = (0u32, 0u32);
            // Premier appel : renvoie PDH_MORE_DATA et la taille nécessaire.
            PdhGetFormattedCounterArrayW(self.counter, PDH_FMT_DOUBLE, &mut size, &mut count, None);
            if size == 0 {
                return None;
            }
            let mut buf = vec![0u8; size as usize];
            let items = buf.as_mut_ptr() as *mut PDH_FMT_COUNTERVALUE_ITEM_W;
            if PdhGetFormattedCounterArrayW(self.counter, PDH_FMT_DOUBLE, &mut size, &mut count, Some(items)) != 0 {
                return None;
            }
            let mut by_engine: HashMap<String, f64> = HashMap::new();
            for it in std::slice::from_raw_parts(items, count as usize) {
                let name = it.szName.to_string().unwrap_or_default();
                let engine = name.rsplit("engtype_").next().unwrap_or("").to_string();
                *by_engine.entry(engine).or_default() += it.FmtValue.Anonymous.doubleValue;
            }
            by_engine
                .values()
                .copied()
                .fold(None, |m: Option<f64>, v| Some(m.map_or(v, |x| x.max(v))))
                .map(|v| v.clamp(0.0, 100.0) as f32)
        }
    }
}

/// Débit réseau agrégé des cartes physiques actives (les cartes virtuelles, VPN et Hyper-V,
/// recompteraient les mêmes octets).
#[derive(Default)]
pub struct Net {
    prev: Option<(u64, u64, std::time::Instant)>,
}

impl Net {
    fn totals() -> Option<(u64, u64)> {
        unsafe {
            let mut table: *mut MIB_IF_TABLE2 = std::ptr::null_mut();
            if GetIfTable2(&mut table).0 != 0 || table.is_null() {
                return None;
            }
            let rows = std::slice::from_raw_parts((*table).Table.as_ptr(), (*table).NumEntries as usize);
            let (mut rx, mut tx) = (0u64, 0u64);
            for r in rows {
                let hardware = r.InterfaceAndOperStatusFlags._bitfield & 1 != 0;
                if hardware && r.Type != IF_TYPE_SOFTWARE_LOOPBACK && r.OperStatus.0 == IF_OPER_STATUS_UP {
                    rx += r.InOctets;
                    tx += r.OutOctets;
                }
            }
            FreeMibTable(table as _);
            Some((rx, tx))
        }
    }

    /// (descendant, montant) en octets par seconde depuis le relevé précédent.
    pub fn sample(&mut self) -> Option<(f64, f64)> {
        let (rx, tx) = Self::totals()?;
        let now = std::time::Instant::now();
        let out = self.prev.and_then(|(prx, ptx, t)| {
            let dt = now.duration_since(t).as_secs_f64();
            (dt > 0.05).then(|| (rx.saturating_sub(prx) as f64 / dt, tx.saturating_sub(ptx) as f64 / dt))
        });
        self.prev = Some((rx, tx, now));
        out
    }
}
