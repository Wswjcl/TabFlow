//! Extract each application's real icon (embedded in its exe resources) and
//! hand it to the frontend as a base64 PNG data URL.
//!
//! Pipeline: process image name → exe full path (toolhelp snapshot) →
//! SHGetFileInfo large icon (HICON) → 32bpp DIB via GetDIBits → PNG.
//! Results are cached per process name (including misses) so the periodic
//! rescans never re-extract.

use base64::Engine;
use std::collections::HashMap;
use std::sync::Mutex;

type Cache = HashMap<String, Option<String>>;
static CACHE: Mutex<Option<Cache>> = Mutex::new(None);

/// Data URL of the given process's icon (e.g. "chrome.exe"), or None if no
/// running process / icon matched. Cached — cheap to call per item per scan.
pub fn process_icon(process_name: &str) -> Option<String> {
    let key = process_name.trim().to_lowercase();
    let mut guard = CACHE.lock().unwrap();
    let cache = guard.get_or_insert_with(HashMap::new);
    if let Some(hit) = cache.get(&key) {
        return hit.clone();
    }
    let icon = extract_icon(&key);
    cache.insert(key, icon.clone());
    icon
}

fn extract_icon(image_name: &str) -> Option<String> {
    find_exe_path(image_name)
        .and_then(|path| shell_icon(&path))
        .and_then(hicon_to_png)
}

/// Full path of the first running process whose image name matches
/// (case-insensitive). Tab items know only the image name — the snapshot
/// walk is what turns that into an exe path.
fn find_exe_path(image_name: &str) -> Option<String> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Diagnostics::ToolHelp::*;

    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0).ok()?;
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };

        let mut found = None;
        let mut ok = Process32FirstW(snapshot, &mut entry).is_ok();
        while ok {
            let len = entry
                .szExeFile
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(entry.szExeFile.len());
            let name = String::from_utf16_lossy(&entry.szExeFile[..len]).to_lowercase();
            if name == image_name {
                if let Some(path) = process_full_path(entry.th32ProcessID) {
                    found = Some(path);
                    break;
                }
            }
            ok = Process32NextW(snapshot, &mut entry).is_ok();
        }

        let _ = CloseHandle(snapshot);
        found
    }
}

fn process_full_path(pid: u32) -> Option<String> {
    use windows::core::PWSTR;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };

    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 512];
        let mut len = buf.len() as u32;
        let result = QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(buf.as_mut_ptr()),
            &mut len,
        );
        let _ = CloseHandle(process);
        if result.is_ok() && len > 0 {
            Some(String::from_utf16_lossy(&buf[..len as usize]))
        } else {
            None
        }
    }
}

/// Large (32×32) icon the shell associates with the file.
fn shell_icon(exe_path: &str) -> Option<windows::Win32::UI::WindowsAndMessaging::HICON> {
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES;
    use windows::Win32::UI::Shell::{SHGetFileInfoW, SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON};

    let wide: Vec<u16> = exe_path.encode_utf16().chain(std::iter::once(0)).collect();
    let mut info = SHFILEINFOW::default();
    let ok = unsafe {
        SHGetFileInfoW(
            PCWSTR(wide.as_ptr()),
            FILE_FLAGS_AND_ATTRIBUTES(0),
            Some(&mut info),
            std::mem::size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON | SHGFI_LARGEICON,
        )
    };
    if ok == 0 {
        None
    } else {
        Some(info.hIcon)
    }
}

/// HICON → top-down 32bpp BGRA → alpha fixup → RGBA → PNG data URL.
fn hicon_to_png(hicon: windows::Win32::UI::WindowsAndMessaging::HICON) -> Option<String> {
    use windows::Win32::Graphics::Gdi::{
        GetDC, GetDIBits, GetObjectW, HGDIOBJ, BITMAP, BITMAPINFO, BITMAPINFOHEADER,
        DIB_RGB_COLORS,
    };
    use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, GetIconInfo, ICONINFO};

    unsafe {
        let mut icon_info = ICONINFO::default();
        if GetIconInfo(hicon, &mut icon_info).is_err() {
            let _ = DestroyIcon(hicon);
            return None;
        }

        let result = (|| -> Option<String> {
            let color = icon_info.hbmColor;
            if color.is_invalid() {
                return None; // monochrome icon — not worth handling
            }

            let mut bmp = BITMAP::default();
            if GetObjectW(
                HGDIOBJ::from(color),
                std::mem::size_of::<BITMAP>() as i32,
                Some(&mut bmp as *mut BITMAP as _),
            ) == 0
            {
                return None;
            }
            let (w, h) = (bmp.bmWidth, bmp.bmHeight);
            if w <= 0 || h <= 0 {
                return None;
            }

            let mut bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: w,
                    biHeight: -h, // top-down rows
                    biPlanes: 1,
                    biBitCount: 32,
                    ..Default::default()
                },
                ..Default::default()
            };

            let mut buf = vec![0u8; (w as usize) * (h as usize) * 4];
            let hdc = GetDC(None);
            let lines = GetDIBits(
                hdc,
                color,
                0,
                h as u32,
                Some(buf.as_mut_ptr() as *mut _),
                &mut bmi as *mut _,
                DIB_RGB_COLORS,
            );
            let _ = windows::Win32::Graphics::Gdi::ReleaseDC(None, hdc);
            if lines != h {
                return None;
            }

            // Legacy icons store no alpha (all 0) but are fully opaque;
            // treat them as such instead of rendering invisible.
            let has_alpha = buf.chunks_exact(4).any(|px| px[3] != 0);
            if !has_alpha {
                for px in buf.chunks_exact_mut(4) {
                    px[3] = 255;
                }
            }

            // BGRA → RGBA
            for px in buf.chunks_exact_mut(4) {
                px.swap(0, 2);
            }

            encode_png(w as u32, h as u32, &buf)
        })();

        let _ = windows::Win32::Graphics::Gdi::DeleteObject(HGDIOBJ::from(icon_info.hbmColor));
        let _ = windows::Win32::Graphics::Gdi::DeleteObject(HGDIOBJ::from(icon_info.hbmMask));
        let _ = DestroyIcon(hicon);
        result
    }
}

fn encode_png(width: u32, height: u32, rgba: &[u8]) -> Option<String> {
    let mut png = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut png, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().ok()?;
        writer.write_image_data(rgba).ok()?;
    }
    let b64 = base64::engine::general_purpose::STANDARD.encode(&png);
    Some(format!("data:image/png;base64,{}", b64))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// End-to-end on a real machine: explorer.exe runs in every interactive
    /// session and owns an icon, so the whole chain (snapshot → exe path →
    /// SHGetFileInfo → GetDIBits → PNG) must produce a decodable data URL.
    #[test]
    fn extracts_a_real_icon_data_url() {
        let icon = process_icon("explorer.exe").expect("explorer.exe icon should extract");
        assert!(icon.starts_with("data:image/png;base64,"));
        let raw = base64::engine::general_purpose::STANDARD
            .decode(icon.trim_start_matches("data:image/png;base64,"))
            .expect("base64 payload must decode");
        assert_eq!(&raw[1..4], b"PNG", "payload must be a PNG");
    }
}
