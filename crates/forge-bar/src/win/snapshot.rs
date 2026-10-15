//! Mode hors écran : `forge-bar --snapshot out.png --theme nord` dessine la barre (ou un panneau)
//! dans une image, avec des données de démonstration fixes. Sert aux aperçus de l'éditeur
//! et aux tests visuels des thèmes en CI. Aucune fenêtre n'est créée.

use super::bar::{Bar, Ctx};
use super::icons;
use super::panel::render_scene;
use super::render::color;
use crate::content;
use crate::widgets::{History, Media, SysData, Wifi};
use chrono::NaiveDate;
use forge_claude::{Day, Session, Summary, Totals};
use forge_config::{
    app_dir, user_themes_dir, ClaudeConfig, Config, Edge, LayoutPreset, Rgba, ScreenConfig, Theme, ThemeStore,
};
use std::collections::HashMap;
use windows::core::HSTRING;
use windows::Win32::Foundation::{GENERIC_WRITE, HWND, RECT};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_GRADIENT_STOP, D2D1_PIXEL_FORMAT, D2D_POINT_2F, D2D_RECT_F,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, ID2D1Factory, ID2D1RenderTarget, D2D1_EXTEND_MODE_CLAMP, D2D1_FACTORY_TYPE_SINGLE_THREADED,
    D2D1_GAMMA_2_2, D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES, D2D1_RENDER_TARGET_PROPERTIES,
};
use windows::Win32::Graphics::DirectWrite::{DWriteCreateFactory, IDWriteFactory, DWRITE_FACTORY_TYPE_SHARED};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_ContainerFormatPng, GUID_WICPixelFormat32bppPBGRA, IWICBitmap, IWICImagingFactory,
    WICBitmapCacheOnDemand, WICBitmapEncoderNoCache,
};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED};

struct Args {
    out: String,
    theme: String,
    width: u32,
    preset: Option<LayoutPreset>,
    edge: Option<Edge>,
    config: Option<String>,
    scene: Option<String>,
    icon_pack: Option<String>,
    all: bool,
    /// Vignette de galerie : peu de widgets, tous lisibles dans une barre étroite.
    thumb: bool,
}

fn parse(args: &[String]) -> Result<Args, String> {
    let mut a = Args {
        out: String::new(),
        theme: "fluent-dark".into(),
        width: 1400,
        preset: None,
        edge: None,
        config: None,
        scene: None,
        icon_pack: None,
        all: false,
        thumb: false,
    };
    let mut it = args.iter();
    while let Some(k) = it.next() {
        let mut val = || it.next().cloned().ok_or(format!("valeur manquante après {k}"));
        match k.as_str() {
            "--snapshot" => a.out = val()?,
            "--snapshot-all" => {
                a.out = val()?;
                a.all = true;
            }
            "--theme" => a.theme = val()?,
            "--width" => a.width = val()?.parse().map_err(|_| "largeur invalide")?,
            "--preset" => {
                let v = val()?;
                a.preset =
                    Some(toml::Value::String(v.clone()).try_into().map_err(|_| format!("préréglage inconnu : {v}"))?);
            }
            "--edge" => {
                let v = val()?;
                a.edge = Some(toml::Value::String(v.clone()).try_into().map_err(|_| format!("bord inconnu : {v}"))?);
            }
            "--config" => a.config = Some(val()?),
            "--scene" => a.scene = Some(val()?),
            "--icon-pack" => a.icon_pack = Some(val()?),
            "--thumb" => a.thumb = true,
            other => return Err(format!("option inconnue : {other}")),
        }
    }
    if a.out.is_empty() {
        return Err("usage : forge-bar --snapshot <sortie.png> [--theme id] [--width px] [--preset p] [--edge e] [--config f] [--scene calendar|claude|clipboard|floating] [--icon-pack nom] | --snapshot-all <dossier>".into());
    }
    if !(200..=4000).contains(&a.width) {
        return Err("largeur hors limites (200 à 4000)".into());
    }
    Ok(a)
}

fn demo_series(base: f32, amp: f32, phase: f32) -> std::collections::VecDeque<f32> {
    (0..60).map(|i| (base + amp * ((i as f32 * 0.35 + phase).sin() * 0.5 + 0.5)).clamp(0.0, 100.0)).collect()
}

fn demo_sys() -> SysData {
    SysData {
        cpu: 37.0,
        ram: 62.0,
        gpu: Some(21.0),
        disk: Some(63.0),
        fps: Some(144),
        fps_denied: false,
        uptime_secs: 3 * 86_400 + 4 * 3600,
        memory: Some((9.8, 16.0)),
        ping_ms: Some(12),
        disk_io: Some(4_200_000.0),
        keyboard: "FR".into(),
        mic_muted: Some(false),
        timer: Some(("12:34".into(), true)),
        custom_out: std::collections::HashMap::new(),
        cpu_temp: Some(54.0),
        gpu_temp: Some(68.0),
        apps: ["Explorateur", "Brave", "Terminal", "Discord"]
            .iter()
            .enumerate()
            .map(|(i, t)| crate::widgets::AppWin {
                hwnd: i as isize + 1,
                title: t.to_string(),
                active: i == 1,
                icon: None,
            })
            .collect(),
        net: Some((1_250_000.0, 82_000.0)),
        battery: Some((78, false)),
        volume: Some((0.55, false)),
        wifi: Some(Wifi { ssid: "Maison".into(), signal: 80 }),
        media: Some(Media { title: "Nocturne op. 9 n° 2".into(), artist: "Chopin".into(), playing: true }),
        weather: Some(crate::weather::Now { temp: 18.4, code: 2, is_day: true, unit: forge_config::TempUnit::Celsius }),
        clipboard: Some(vec![
            "cargo build --release".into(),
            "https://github.com/Heiphaistos".into(),
            "Réunion de 14 h".into(),
        ]),
        clock: "14:32".into(),
        hist: History {
            cpu: demo_series(15.0, 60.0, 0.0),
            ram: demo_series(55.0, 12.0, 1.0),
            gpu: demo_series(5.0, 40.0, 2.0),
            net: demo_series(10_000.0, 90_000.0, 3.0).into_iter().map(|v| v * 12.0).collect(),
        },
    }
}

fn demo_claude() -> Summary {
    let day = |d, c: f64| Day {
        date: NaiveDate::from_ymd_opt(2026, 10, d).unwrap(),
        cost_usd: c,
        tokens: (c * 90_000.0) as u64,
    };
    Summary {
        today: Totals { cost_usd: 1.84, input: 120_000, output: 40_000, cache_read: 900_000, ..Totals::default() },
        week: Totals { cost_usd: 9.2, input: 600_000, output: 200_000, cache_read: 4_000_000, ..Totals::default() },
        month: Totals {
            cost_usd: 41.3,
            input: 2_600_000,
            output: 800_000,
            cache_read: 18_000_000,
            ..Totals::default()
        },
        session: Some(Session {
            id: "demo".into(),
            model: "claude-sonnet-5-5".into(),
            context_tokens: 84_000,
            context_pct: 42.0,
            last: chrono::Utc::now(),
        }),
        daily: (2..16)
            .map(|d| day(d, [0.4, 1.2, 2.8, 0.0, 0.0, 3.1, 4.4, 2.2, 1.9, 5.0, 3.3, 0.6, 2.0, 1.84][d as usize - 2]))
            .collect(),
        models: vec![
            ("claude-sonnet-5-5".into(), Totals { cost_usd: 30.1, input: 2_000_000, ..Totals::default() }),
            ("claude-opus-5-5".into(), Totals { cost_usd: 11.2, input: 400_000, ..Totals::default() }),
        ],
        five_hour: Some(forge_claude::subscription::Block {
            start: chrono::Utc::now() - chrono::Duration::hours(2),
            end: chrono::Utc::now() + chrono::Duration::hours(3),
            cost_usd: 3.4,
            limit_usd: Some(8.0),
            pct: Some(42.5),
        }),
        files_scanned: 12,
        updated: Some(chrono::Local::now()),
        ..Summary::default()
    }
}

/// Fond de démonstration coloré : on y voit la transparence des thèmes en verre.
fn backdrop(rt: &ID2D1RenderTarget, w: f32, h: f32) -> windows::core::Result<()> {
    unsafe {
        let stops = [
            D2D1_GRADIENT_STOP { position: 0.0, color: color(Rgba { r: 0.10, g: 0.25, b: 0.55, a: 1.0 }) },
            D2D1_GRADIENT_STOP { position: 0.5, color: color(Rgba { r: 0.75, g: 0.45, b: 0.30, a: 1.0 }) },
            D2D1_GRADIENT_STOP { position: 1.0, color: color(Rgba { r: 0.35, g: 0.15, b: 0.55, a: 1.0 }) },
        ];
        let coll = rt.CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)?;
        let props = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES {
            startPoint: D2D_POINT_2F { x: 0.0, y: 0.0 },
            endPoint: D2D_POINT_2F { x: w, y: h.max(w * 0.2) },
        };
        let brush = rt.CreateLinearGradientBrush(&props, None, &coll)?;
        rt.FillRectangle(&D2D_RECT_F { left: 0.0, top: 0.0, right: w, bottom: h }, &brush);
    }
    Ok(())
}

struct Env {
    d2d: ID2D1Factory,
    dwrite: IDWriteFactory,
    wic: IWICImagingFactory,
}

fn env() -> Result<Env, String> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        Ok(Env {
            d2d: D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None).map_err(|e| e.to_string())?,
            dwrite: DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED).map_err(|e| e.to_string())?,
            wic: CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).map_err(|e| e.to_string())?,
        })
    }
}

/// Dessine dans une image PBGRA `w` x `h` via `draw` et renvoie le bitmap.
fn render(
    env: &Env,
    w: u32,
    h: u32,
    draw: impl FnOnce(&ID2D1RenderTarget) -> windows::core::Result<()>,
) -> Result<IWICBitmap, String> {
    let e = |e: windows::core::Error| e.to_string();
    unsafe {
        let bmp = env.wic.CreateBitmap(w, h, &GUID_WICPixelFormat32bppPBGRA, WICBitmapCacheOnDemand).map_err(e)?;
        let props = D2D1_RENDER_TARGET_PROPERTIES {
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
            },
            ..Default::default()
        };
        let rt = env.d2d.CreateWicBitmapRenderTarget(&bmp, &props).map_err(e)?;
        rt.BeginDraw();
        let drawn = draw(&rt);
        let ended = rt.EndDraw(None, None);
        drawn.map_err(e)?;
        ended.map_err(e)?;
        Ok(bmp)
    }
}

fn save_png(env: &Env, bmp: &IWICBitmap, w: u32, h: u32, path: &str) -> Result<(), String> {
    let e = |e: windows::core::Error| e.to_string();
    unsafe {
        let encoder = env.wic.CreateEncoder(&GUID_ContainerFormatPng, std::ptr::null()).map_err(e)?;
        let stream = env.wic.CreateStream().map_err(e)?;
        stream.InitializeFromFilename(&HSTRING::from(path), GENERIC_WRITE.0).map_err(e)?;
        encoder.Initialize(&stream, WICBitmapEncoderNoCache).map_err(e)?;
        let (mut frame, mut bag) = (None, None);
        encoder.CreateNewFrame(&mut frame, &mut bag).map_err(e)?;
        let frame = frame.ok_or("image sans cadre")?;
        frame.Initialize(None).map_err(e)?;
        frame.SetSize(w, h).map_err(e)?;
        let mut fmt = GUID_WICPixelFormat32bppPBGRA;
        frame.SetPixelFormat(&mut fmt).map_err(e)?;
        frame.WriteSource(bmp, std::ptr::null()).map_err(e)?;
        frame.Commit().map_err(e)?;
        encoder.Commit().map_err(e)?;
    }
    Ok(())
}

fn pixels(bmp: &IWICBitmap, w: u32, h: u32) -> Vec<u8> {
    let mut buf = vec![0u8; (w * h * 4) as usize];
    unsafe {
        let _ = bmp.CopyPixels(std::ptr::null(), w * 4, &mut buf);
    }
    buf
}

/// Part des pixels qui diffèrent nettement du fond de la barre : du texte et des icônes ont été dessinés.
fn ink_ratio(px: &[u8]) -> f32 {
    let mut counts: HashMap<[u8; 4], usize> = HashMap::new();
    for p in px.chunks_exact(4) {
        *counts.entry([p[0] / 8, p[1] / 8, p[2] / 8, p[3] / 8]).or_default() += 1;
    }
    let total = px.len() / 4;
    let top = counts.values().copied().max().unwrap_or(0);
    1.0 - top as f32 / total.max(1) as f32
}

struct Job<'a> {
    env: &'a Env,
    cfg: &'a Config,
    theme: Theme,
    width: u32,
    scene: Option<&'a str>,
    pack_name: &'a str,
}

/// Rend un thème ; renvoie (largeur, hauteur, pixels) et écrit le PNG si `out` n'est pas vide.
fn render_job(job: &Job, out: &str) -> Result<(u32, u32, Vec<u8>), String> {
    let (env, cfg) = (job.env, job.cfg);
    let sys = demo_sys();
    let claude = demo_claude();
    let pack = icons::load(job.pack_name, &env.d2d);
    let (w, h, bmp) = if let Some(kind) = job.scene {
        let scene = match kind {
            "calendar" => {
                let today = NaiveDate::from_ymd_opt(2026, 10, 15).unwrap();
                content::calendar(&crate::calendar::month_grid(2026, 10, today))
            }
            "claude" => content::claude_detail(&claude, &cfg.claude),
            "clipboard" => content::clipboard(sys.clipboard.as_deref().unwrap_or(&[]), 60),
            "floating" => content::floating(&claude, &cfg.claude),
            other => return Err(format!("scène inconnue : {other}")),
        };
        let (w, h) = (scene.w.ceil() as u32, scene.h.ceil() as u32);
        let mut cache = HashMap::new();
        let bmp = render(env, w, h, |rt| {
            backdrop(rt, w as f32, h as f32)?;
            render_scene(rt, &env.dwrite, &job.theme, &scene, 1.0, false, &mut cache)
        })?;
        (w, h, bmp)
    } else {
        let screen: ScreenConfig = cfg.screen[0].clone();
        let rows = screen.layout.rows().len() as u32;
        let vertical = screen.edge.is_vertical();
        let thick = if rows > 1 && !vertical { screen.thickness.max(52) } else { screen.thickness };
        let (w, h) = if vertical { (thick, job.width.min(900)) } else { (job.width, thick) };
        let monitor = RECT { left: 0, top: 0, right: w as i32, bottom: h as i32 };
        let mut bar = Bar::new(HWND::default(), screen, job.theme.clone(), monitor, 96);
        let icon_font = "Segoe Fluent Icons";
        let claude_cfg: ClaudeConfig = cfg.claude.clone();
        let bmp = render(env, w, h, |rt| {
            backdrop(rt, w as f32, h as f32)?;
            let ctx = Ctx {
                d2d: &env.d2d,
                dwrite: &env.dwrite,
                icon_font,
                sys: &sys,
                claude: &claude,
                claude_cfg: &claude_cfg,
                custom: &cfg.custom,
                pack: &pack,
            };
            bar.draw(rt, &ctx, w as f32, h as f32)
        })?;
        (w, h, bmp)
    };
    if !out.is_empty() {
        save_png(env, &bmp, w, h, out)?;
    }
    Ok((w, h, pixels(&bmp, w, h)))
}

fn load_config(path: &Option<String>, a: &Args) -> Result<Config, String> {
    let mut cfg = match path {
        Some(p) => Config::from_toml(&std::fs::read_to_string(p).map_err(|e| format!("{p} : {e}"))?)
            .map_err(|e| e.to_string())?,
        None => Config::default(),
    };
    let s = &mut cfg.screen[0];
    if let Some(p) = a.preset {
        s.layout.preset = p;
    }
    if let Some(e) = a.edge {
        s.edge = e;
    }
    if a.thumb {
        let v = |ids: &[&str]| ids.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        s.layout.left = v(&["desktop"]);
        s.layout.center = v(&["claude"]);
        s.layout.right = v(&["cpu", "wifi", "clock"]);
        cfg.claude.bar_summary = v(&["context"]);
    }
    Ok(cfg)
}

/// Point d'entrée : renvoie le code de sortie du processus.
pub fn run(args: &[String]) -> i32 {
    match run_inner(args) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("forge-bar : {e}");
            1
        }
    }
}

fn run_inner(args: &[String]) -> Result<(), String> {
    let a = parse(args)?;
    let env = env()?;
    let mut cfg = load_config(&a.config, &a)?;
    if let Some(p) = &a.icon_pack {
        cfg.general.icon_pack = p.clone();
    }
    let themes = ThemeStore::load(Some(&user_themes_dir()));
    let _ = app_dir();
    if !a.all {
        if !themes.ids().contains(&a.theme) {
            return Err(format!("thème inconnu : {}", a.theme));
        }
        let job = Job {
            env: &env,
            cfg: &cfg,
            theme: themes.get(&a.theme).adapted(&super::wallpaper::env(true)),
            width: a.width,
            scene: a.scene.as_deref(),
            pack_name: &cfg.general.icon_pack,
        };
        render_job(&job, &a.out)?;
        return Ok(());
    }
    std::fs::create_dir_all(&a.out).map_err(|e| e.to_string())?;
    let mut failed = Vec::new();
    let mut report = String::new();
    for id in themes.ids() {
        for (suffix, edge) in [("", None), ("-vertical", Some(Edge::Left))] {
            let mut c = cfg.clone();
            if let Some(e) = edge {
                c.screen[0].edge = e;
                c.screen[0].thickness = 64;
            }
            let job = Job {
                env: &env,
                cfg: &c,
                theme: themes.get(&id).adapted(&super::wallpaper::env(true)),
                width: a.width,
                scene: None,
                pack_name: &c.general.icon_pack,
            };
            let file = format!("{}/{id}{suffix}.png", a.out);
            let (w, h, px) = render_job(&job, &file)?;
            let ink = ink_ratio(&px);
            let ok = ink > 0.02;
            let line = format!("{} {id}{suffix} {w}x{h} encre {:.1} %", if ok { "ok  " } else { "VIDE" }, ink * 100.0);
            println!("{line}");
            report.push_str(&line);
            report.push(char::from(10));
            if !ok {
                failed.push(format!("{id}{suffix}"));
            }
        }
    }
    // Sous le sous-système Windows la console n'est pas rattachée : le rapport est aussi écrit dans un fichier.
    std::fs::write(format!("{}/report.txt", a.out), report).map_err(|e| e.to_string())?;
    if failed.is_empty() {
        Ok(())
    } else {
        Err(format!("rendu vide pour : {}", failed.join(", ")))
    }
}
