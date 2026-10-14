//! Couleur dominante du fond d'écran, pour remplacer l'accent du thème.

use forge_config::Rgba;
use windows::core::{w, HSTRING};
use windows::Win32::Foundation::GENERIC_READ;
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_WICPixelFormat32bppBGRA, IWICImagingFactory, WICBitmapDitherTypeNone,
    WICBitmapInterpolationModeFant, WICBitmapPaletteTypeCustom, WICDecodeMetadataCacheOnDemand,
};
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_SZ};

fn wallpaper_path() -> Option<String> {
    let mut buf = [0u16; 520];
    let mut size = (buf.len() * 2) as u32;
    let rc = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Control Panel\\Desktop"),
            w!("WallPaper"),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr() as _),
            Some(&mut size),
        )
    };
    if rc.is_err() {
        return None;
    }
    let n = (size as usize / 2).saturating_sub(1).min(buf.len());
    let p = String::from_utf16_lossy(&buf[..n]);
    if !p.is_empty() && std::path::Path::new(&p).exists() {
        return Some(p);
    }
    // Diaporama ou chemin vide : Windows garde l'image courante ici.
    let t =
        std::path::PathBuf::from(std::env::var("APPDATA").ok()?).join("Microsoft/Windows/Themes/TranscodedWallpaper");
    t.exists().then(|| t.display().to_string())
}

/// Lit le fond d'écran réduit à 48x48 pixels BGRA.
fn thumbnail(path: &str) -> windows::core::Result<Vec<u8>> {
    unsafe {
        let factory: IWICImagingFactory = CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
        let decoder = factory.CreateDecoderFromFilename(
            &HSTRING::from(path),
            None,
            GENERIC_READ,
            WICDecodeMetadataCacheOnDemand,
        )?;
        let frame = decoder.GetFrame(0)?;
        let scaler = factory.CreateBitmapScaler()?;
        scaler.Initialize(&frame, 48, 48, WICBitmapInterpolationModeFant)?;
        let conv = factory.CreateFormatConverter()?;
        conv.Initialize(
            &scaler,
            &GUID_WICPixelFormat32bppBGRA,
            WICBitmapDitherTypeNone,
            None,
            0.0,
            WICBitmapPaletteTypeCustom,
        )?;
        let mut px = vec![0u8; 48 * 48 * 4];
        conv.CopyPixels(std::ptr::null(), 48 * 4, &mut px)?;
        Ok(px)
    }
}

/// Couleur d'accent tirée du fond d'écran actuel ; `None` s'il est illisible.
pub fn accent() -> Option<Rgba> {
    let px = thumbnail(&wallpaper_path()?).ok()?;
    crate::dominant::dominant(&px)
}
