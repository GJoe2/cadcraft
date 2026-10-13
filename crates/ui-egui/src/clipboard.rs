//! The system clipboard: Cut/Copy/Paste key presses, and the drawing clipboard mirrored as DXF
//! text so it can be pasted into another CADCraft window.
//!
//! egui turns Cmd/Ctrl+C, X and V into `Copy`, `Cut` and `Paste(text)` events instead of key
//! presses, so the clipboard shortcuts are read from those. A `Paste` arrives only when the system
//! clipboard holds text; every copy therefore puts its DXF text there.

use std::hash::{DefaultHasher, Hash, Hasher};

use cadcraft_engine::cmd::clipboard;

use crate::CadApp;

/// egui memory: the clipboard copy last mirrored, and the hash of the text it became.
fn serial_id() -> egui::Id {
    egui::Id::new("cc_clipboard_serial")
}
fn text_id() -> egui::Id {
    egui::Id::new("cc_clipboard_text")
}

/// Line endings normalised (pasted text arrives with `\n`), so our own text is recognised.
fn hash(text: &str) -> u64 {
    let mut h = DefaultHasher::new();
    text.replace("\r\n", "\n").hash(&mut h);
    h.finish()
}

/// Put a new copy (COPYCLIP, CUTCLIP, COPYBASE from anywhere) on the system clipboard, once.
pub fn mirror(app: &CadApp, ctx: &egui::Context) {
    let Some(src) = &app.session.clipboard_source else { return };
    if src.uid.is_none() || ctx.data(|d| d.get_temp::<u64>(serial_id())) == Some(src.serial) {
        return;
    }
    ctx.data_mut(|d| d.insert_temp(serial_id(), src.serial));
    if let Some(text) = clipboard::system_text(&app.session) {
        ctx.data_mut(|d| d.insert_temp(text_id(), hash(&text)));
        ctx.copy_text(text);
    }
}

/// The commands the Cut/Copy/Paste events of this frame ask for. Pasted DXF text that isn't our
/// own last copy (another CADCraft's) becomes the clipboard first.
pub fn shortcut_commands(app: &mut CadApp, ctx: &egui::Context) -> Vec<&'static str> {
    let (events, shift) = ctx.input(|i| {
        let ev: Vec<egui::Event> =
            i.events.iter().filter(|e| matches!(e, egui::Event::Copy | egui::Event::Cut | egui::Event::Paste(_))).cloned().collect();
        (ev, i.modifiers.shift)
    });
    let mut out = Vec::new();
    for e in events {
        match e {
            egui::Event::Copy => out.push(if shift { "copybase" } else { "copyclip" }),
            egui::Event::Cut => out.push("cutclip"),
            egui::Event::Paste(text) => {
                let h = hash(&text);
                let loaded_text = if ctx.data(|d| d.get_temp::<u64>(text_id())) != Some(h) {
                    let ok = clipboard::load_system_text(&mut app.session, &text);
                    if ok {
                        ctx.data_mut(|d| d.insert_temp(text_id(), h));
                    }
                    ok
                } else {
                    !app.session.clipboard.is_empty()
                };
                if !loaded_text {
                    try_load_os_clipboard_image(app);
                }
                out.push(if shift { "pasteblock" } else { "pasteclip" });
            }
            _ => {}
        }
    }
    if out.is_empty() {
        let v_released = ctx.input(|i| {
            i.events.iter().any(|e| matches!(e, egui::Event::Key { key: egui::Key::V, pressed: false, modifiers, .. } if modifiers.command))
        });
        if v_released && try_load_os_clipboard_image(app) {
            out.push(if shift { "pasteblock" } else { "pasteclip" });
        }
    }
    out
}

/// Try to load a raster image from the OS clipboard into the session clipboard.
#[cfg(not(target_arch = "wasm32"))]
pub fn try_load_os_clipboard_image(app: &mut CadApp) -> bool {
    if let Ok(mut cb) = arboard::Clipboard::new()
        && let Ok(img) = cb.get_image()
    {
        let width = img.width as u32;
        let height = img.height as u32;
        if width > 0
            && height > 0
            && !img.bytes.is_empty()
            && let Some(rgba_img) = image::RgbaImage::from_raw(width, height, img.bytes.into_owned())
        {
            let mut png_bytes = Vec::new();
            if image::DynamicImage::ImageRgba8(rgba_img).write_to(&mut std::io::Cursor::new(&mut png_bytes), image::ImageFormat::Png).is_ok() {
                app.session.set_clipboard_image(png_bytes, width, height);
                return true;
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        if let Some((png_bytes, width, height)) = try_load_windows_clipboard_image() {
            app.session.set_clipboard_image(png_bytes, width, height);
            return true;
        }
    }

    #[cfg(target_os = "linux")]
    {
        if let Some((png_bytes, width, height)) = try_load_xclip_image() {
            app.session.set_clipboard_image(png_bytes, width, height);
            return true;
        }
        if let Some((png_bytes, width, height)) = try_load_wsl_clipboard_image() {
            app.session.set_clipboard_image(png_bytes, width, height);
            return true;
        }
    }

    false
}

#[cfg(target_os = "windows")]
fn try_load_windows_clipboard_image() -> Option<(Vec<u8>, u32, u32)> {
    use clipboard_win::{Clipboard, Getter, formats};

    let _clip = Clipboard::new_attempts(10).ok()?;

    // 1. Try standard CF_BITMAP (GDI synthesizes CF_BITMAP for any image or screenshot format)
    let mut bmp_bytes = Vec::new();
    if formats::Bitmap.read_clipboard(&mut bmp_bytes).is_ok()
        && !bmp_bytes.is_empty()
        && let Ok(dyn_img) = image::load_from_memory(&bmp_bytes)
    {
        let (w, h) = (dyn_img.width(), dyn_img.height());
        if w > 0 && h > 0 && w <= 32768 && h <= 32768 {
            let mut png_bytes = Vec::new();
            if dyn_img.write_to(&mut std::io::Cursor::new(&mut png_bytes), image::ImageFormat::Png).is_ok() {
                return Some((png_bytes, w, h));
            }
        }
    }

    // 2. Fallback to raw CF_DIB (standard format 8)
    let mut dib_bytes = Vec::new();
    if formats::RawData(formats::CF_DIB).read_clipboard(&mut dib_bytes).is_ok() && dib_bytes.len() >= 40 {
        let header_size = u32::from_le_bytes([*dib_bytes.first()?, *dib_bytes.get(1)?, *dib_bytes.get(2)?, *dib_bytes.get(3)?]);
        let w = i32::from_le_bytes([*dib_bytes.get(4)?, *dib_bytes.get(5)?, *dib_bytes.get(6)?, *dib_bytes.get(7)?]).abs();
        let h = i32::from_le_bytes([*dib_bytes.get(8)?, *dib_bytes.get(9)?, *dib_bytes.get(10)?, *dib_bytes.get(11)?]).abs();
        let bit_count = u16::from_le_bytes([*dib_bytes.get(14)?, *dib_bytes.get(15)?]);
        if header_size >= 40 && w > 0 && h > 0 && w <= 32768 && h <= 32768 {
            let clr_used = u32::from_le_bytes([*dib_bytes.get(32)?, *dib_bytes.get(33)?, *dib_bytes.get(34)?, *dib_bytes.get(35)?]);
            let palette_colors = if clr_used > 0 {
                (clr_used as usize).min(256)
            } else if bit_count <= 8 {
                1usize << bit_count
            } else {
                0
            };
            let bf_off_bits = 14 + header_size + (palette_colors * 4) as u32;
            let file_size = 14 + dib_bytes.len() as u32;
            let mut bmp = Vec::with_capacity(14 + dib_bytes.len());
            bmp.extend_from_slice(b"BM");
            bmp.extend_from_slice(&file_size.to_le_bytes());
            bmp.extend_from_slice(&[0, 0, 0, 0]);
            bmp.extend_from_slice(&bf_off_bits.to_le_bytes());
            bmp.extend_from_slice(&dib_bytes);
            if let Ok(dyn_img) = image::load_from_memory(&bmp) {
                let (w, h) = (dyn_img.width(), dyn_img.height());
                let mut png_bytes = Vec::new();
                if dyn_img.write_to(&mut std::io::Cursor::new(&mut png_bytes), image::ImageFormat::Png).is_ok() {
                    return Some((png_bytes, w, h));
                }
            }
        }
    }

    None
}

#[cfg(target_os = "linux")]
fn is_wsl() -> bool {
    std::path::Path::new("/proc/sys/fs/binfmt_misc/WSLInterop").exists()
        || std::env::var_os("WSL_DISTRO_NAME").is_some()
        || std::env::var_os("WSL_INTEROP").is_some()
}

#[cfg(target_os = "linux")]
fn try_load_xclip_image() -> Option<(Vec<u8>, u32, u32)> {
    let output = std::process::Command::new("xclip")
        .args(["-selection", "clipboard", "-t", "image/png", "-o"])
        .stdin(std::process::Stdio::null())
        .output()
        .ok()?;
    if output.status.success() && output.stdout.starts_with(b"\x89PNG\r\n\x1a\n") {
        let dyn_img = image::load_from_memory(&output.stdout).ok()?;
        let (w, h) = (dyn_img.width(), dyn_img.height());
        if w > 0 && h > 0 && w <= 16384 && h <= 16384 {
            return Some((output.stdout, w, h));
        }
    }
    None
}

#[cfg(target_os = "linux")]
fn decode_base64(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let mut buf = 0u32;
    let mut bits = 0;
    for &b in s.as_bytes() {
        let val = match b {
            b'A'..=b'Z' => b - b'A',
            b'a'..=b'z' => b - b'a' + 26,
            b'0'..=b'9' => b - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' | b'\r' | b'\n' | b' ' => continue,
            _ => return None,
        };
        buf = (buf << 6) | (val as u32);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
            buf &= (1 << bits) - 1;
        }
    }
    Some(out)
}

#[cfg(target_os = "linux")]
fn try_load_wsl_clipboard_image() -> Option<(Vec<u8>, u32, u32)> {
    if !is_wsl() {
        return None;
    }
    let ps_paths =
        ["/mnt/c/Windows/System32/WindowsPowerShell/v1.0/powershell.exe", "/mnt/c/Program Files/PowerShell/7/pwsh.exe", "powershell.exe", "pwsh.exe"];
    let ps = ps_paths.iter().find(|p| if p.starts_with('/') { std::path::Path::new(p).exists() } else { true })?;

    let script = r#"Add-Type -AssemblyName System.Windows.Forms; Add-Type -AssemblyName System.Drawing; $cb = [System.Windows.Forms.Clipboard]::GetDataObject(); if ($cb -ne $null) { if ($cb.GetDataPresent('PNG')) { $st = $cb.GetData('PNG'); if ($st -is [System.IO.MemoryStream]) { [System.Convert]::ToBase64String($st.ToArray()) } elseif ($st -is [byte[]]) { [System.Convert]::ToBase64String($st) } } elseif ($cb.GetDataPresent([System.Windows.Forms.DataFormats]::Bitmap)) { $img = [System.Windows.Forms.Clipboard]::GetImage(); if ($img -ne $null) { $ms = New-Object System.IO.MemoryStream; $img.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png); [System.Convert]::ToBase64String($ms.ToArray()) } } }"#;

    let mut cmd = std::process::Command::new(ps);
    if std::path::Path::new("/mnt/c").exists() {
        cmd.current_dir("/mnt/c");
    } else {
        cmd.current_dir(std::env::temp_dir());
    }
    let output =
        cmd.arg("-Sta").arg("-NoProfile").arg("-NonInteractive").arg("-Command").arg(script).stdin(std::process::Stdio::null()).output().ok()?;

    if !output.status.success() {
        return None;
    }

    let b64 = String::from_utf8(output.stdout).ok()?;
    let b64 = b64.trim();
    if b64.is_empty() {
        return None;
    }

    let png_bytes = decode_base64(b64)?;
    if png_bytes.len() < 24 || !png_bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return None;
    }

    let dyn_img = image::load_from_memory(&png_bytes).ok()?;
    let (w, h) = (dyn_img.width(), dyn_img.height());
    if w == 0 || h == 0 || w > 16384 || h > 16384 {
        return None;
    }

    Some((png_bytes, w, h))
}

#[cfg(target_arch = "wasm32")]
pub fn try_load_os_clipboard_image(_app: &mut CadApp) -> bool {
    false
}
