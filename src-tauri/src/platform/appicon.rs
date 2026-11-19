//! The real icon of the app that plays music, as Windows itself draws it.
//!
//! GSMTC only gives an AppUserModelID (`ru.yandex.desktop.music`,
//! `Spotify.exe`, `MSEdge`, `…!App` for Store apps). The shell resolves most
//! of those through `shell:AppsFolder`; when it can't (a bare `*.exe` id), we
//! find a running process with that name and ask the shell for its exe's
//! icon. The result is a small PNG data URL, cached per id for the session.

use std::collections::HashMap;
use std::io::Cursor;

use base64::Engine;
use parking_lot::Mutex;
use windows::core::HSTRING;
use windows::Win32::Foundation::SIZE;
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, DeleteDC, DeleteObject, GetDIBits, GetObjectW, BITMAP, BITMAPINFO, BITMAPINFOHEADER,
    BI_RGB, DIB_RGB_COLORS, HBITMAP, HGDIOBJ,
};
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};
use windows::Win32::UI::Shell::{
    FOLDERID_AppsFolder, IShellItemImageFactory, SHCreateItemFromParsingName, SHCreateItemInKnownFolder,
    KF_FLAG_DEFAULT, SIIGBF_BIGGERSIZEOK, SIIGBF_ICONONLY,
};

const PX: i32 = 64;

static CACHE: Mutex<Option<HashMap<String, Option<String>>>> = Mutex::new(None);

/// PNG data URL of the source app's icon, or None when Windows has none.
pub fn app_icon(aumid: &str) -> Option<String> {
    let aumid = aumid.trim();
    if aumid.is_empty() {
        return None;
    }
    if let Some(hit) = CACHE.lock().as_ref().and_then(|c| c.get(aumid).cloned()) {
        return hit;
    }
    // The shell's image factory is happiest on an STA thread of its own.
    let id = aumid.to_string();
    let out = std::thread::spawn(move || unsafe {
        let hr = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let r = resolve(&id);
        if hr.is_ok() {
            CoUninitialize();
        }
        r
    })
    .join()
    .ok()
    .flatten();
    CACHE.lock().get_or_insert_with(HashMap::new).insert(aumid.to_string(), out.clone());
    out
}

unsafe fn resolve(aumid: &str) -> Option<String> {
    let factory: Option<IShellItemImageFactory> =
        SHCreateItemInKnownFolder(&FOLDERID_AppsFolder, KF_FLAG_DEFAULT, &HSTRING::from(aumid)).ok();
    let factory = factory.or_else(|| {
        let path = exe_path_for(aumid)?;
        SHCreateItemFromParsingName(&HSTRING::from(path), None).ok()
    })?;
    let hbmp = factory.GetImage(SIZE { cx: PX, cy: PX }, SIIGBF_ICONONLY | SIIGBF_BIGGERSIZEOK).ok()?;
    let png = bitmap_png(hbmp);
    let _ = DeleteObject(HGDIOBJ(hbmp.0));
    png
}

/// `Spotify.exe` / `spotify` → the full path of a running process with that name.
fn exe_path_for(aumid: &str) -> Option<String> {
    let want = aumid.split('!').next().unwrap_or(aumid).to_lowercase();
    let want = want.trim_end_matches(".exe");
    super::winmgr::list_windows()
        .into_iter()
        .find(|w| w.process.to_lowercase().trim_end_matches(".exe") == want)
        .and_then(|w| super::process::process_path(w.pid))
}

unsafe fn bitmap_png(hbmp: HBITMAP) -> Option<String> {
    let mut bm = BITMAP::default();
    let got = GetObjectW(HGDIOBJ(hbmp.0), std::mem::size_of::<BITMAP>() as i32, Some(&mut bm as *mut _ as *mut _));
    if got == 0 || bm.bmWidth <= 0 || bm.bmHeight == 0 {
        return None;
    }
    let (w, h) = (bm.bmWidth, bm.bmHeight.abs());
    let mut bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w,
            biHeight: -h,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut buf = vec![0u8; (w * h * 4) as usize];
    let dc = CreateCompatibleDC(None);
    let lines = GetDIBits(dc, hbmp, 0, h as u32, Some(buf.as_mut_ptr() as *mut _), &mut bmi, DIB_RGB_COLORS);
    let _ = DeleteDC(dc);
    if lines == 0 {
        return None;
    }
    // BGRA, premultiplied → straight RGBA. An all-zero alpha means an old
    // icon without a mask: treat it as opaque.
    let any_alpha = buf.chunks_exact(4).any(|p| p[3] != 0);
    for px in buf.chunks_exact_mut(4) {
        px.swap(0, 2);
        if !any_alpha {
            px[3] = 255;
        } else if px[3] > 0 && px[3] < 255 {
            let a = px[3] as u32;
            for c in &mut px[..3] {
                *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
    let img = image::RgbaImage::from_raw(w as u32, h as u32, buf)?;
    let mut out = Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).ok()?;
    Some(format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(out.into_inner())
    ))
}

#[cfg(test)]
mod live {
    /// `cargo test --lib live_app_icon -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn live_app_icon() {
        for id in ["ru.yandex.desktop.music", "Spotify.exe", "MSEdge", "Chrome"] {
            let r = super::app_icon(id);
            if let (Some(url), Ok(dir)) = (r.as_ref(), std::env::var("ICON_DUMP")) {
                use base64::Engine;
                let b = base64::engine::general_purpose::STANDARD.decode(url.split(',').nth(1).unwrap()).unwrap();
                std::fs::write(format!("{dir}/{}.png", id.replace(['.', '!'], "_")), b).unwrap();
            }
            println!("{id}: {}", r.as_ref().map(|s| s.len()).map_or("none".to_string(), |n| format!("{n} chars")));
        }
    }
}
