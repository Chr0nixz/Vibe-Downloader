use crate::platform;
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Serialize, specta::Type)]
pub struct DiskSpaceInfo {
    pub path: String,
    // Stored as String to avoid Specta's BigInt restriction on u64. The
    // frontend parses with Number() for arithmetic and display.
    pub total_bytes: String,
    pub available_bytes: String,
}

#[derive(Debug, Serialize, specta::Type)]
pub struct SystemFileIcon {
    /// PNG-encoded icon as a base64 data URL (`data:image/png;base64,...`).
    /// `None` when the OS has no associated icon for the extension.
    pub data_url: Option<String>,
    /// Best-effort MIME-type hint for the extension (e.g. `video/mp4`).
    /// Useful as a fallback label when no icon is available.
    pub mime_hint: Option<String>,
}

/// Query total and available disk space for the volume that contains `path`.
/// Returns a clear error string when the path does not exist or the underlying
/// OS call fails, so the frontend can render a helpful toast.
#[tauri::command]
#[specta::specta]
pub async fn query_disk_space(path: String) -> Result<DiskSpaceInfo, String> {
    let path = Path::new(&path);
    // Walk up to the first existing ancestor so a not-yet-created save dir
    // still yields a meaningful volume reading instead of an error.
    let mut probe = path;
    while !probe.exists() {
        match probe.parent() {
            Some(parent) => probe = parent,
            None => return Err("path does not exist and has no parent".to_string()),
        }
    }

    let total = fs4::free_space(probe).map_err(|e| format!("failed to read disk space: {e}"))?;
    let available =
        fs4::available_space(probe).map_err(|e| format!("failed to read available space: {e}"))?;

    Ok(DiskSpaceInfo {
        path: probe.to_string_lossy().to_string(),
        total_bytes: total.to_string(),
        available_bytes: available.to_string(),
    })
}

#[tauri::command]
#[specta::specta]
pub async fn request_system_shutdown() -> Result<(), String> {
    platform::shutdown_now()
}

#[tauri::command]
#[specta::specta]
pub async fn request_system_sleep() -> Result<(), String> {
    platform::sleep_now()
}

#[tauri::command]
#[specta::specta]
pub async fn request_system_hibernate() -> Result<(), String> {
    platform::hibernate_now()
}

#[tauri::command]
#[specta::specta]
pub async fn request_lock_screen() -> Result<(), String> {
    platform::lock_screen_now()
}

/// Extract the OS-associated file-type icon for a file name.
///
/// The file does **not** need to exist on disk — the icon is resolved purely
/// from the extension's system association:
///
/// - **Windows**: `SHGetFileInfo` with `SHGFI_USEFILEATTRIBUTES`
/// - **macOS**: `NSWorkspace.iconForFileType:`
/// - **Linux**: freedesktop icon theme lookup via the `freedesktop` crate
///
/// Returns a PNG-encoded base64 data URL suitable for `<img>`, or `None`
/// when extraction fails so the frontend can fall back to a generic icon.
#[tauri::command]
#[specta::specta]
pub async fn extract_system_file_icon(file_name: String) -> Result<SystemFileIcon, String> {
    let ext = Path::new(&file_name)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .filter(|e| !e.is_empty());

    let mime_hint = ext.as_deref().and_then(mime_for_ext).map(String::from);

    let data_url = match &ext {
        Some(ext) => extract_icon(&file_name, ext),
        None => None,
    };

    Ok(SystemFileIcon {
        data_url,
        mime_hint,
    })
}

// ── Platform dispatch ──────────────────────────────────────────────────────

#[cfg(windows)]
fn extract_icon(file_name: &str, _ext: &str) -> Option<String> {
    extract_icon_windows(file_name)
}

#[cfg(target_os = "macos")]
fn extract_icon(_file_name: &str, ext: &str) -> Option<String> {
    extract_icon_macos(ext)
}

#[cfg(target_os = "linux")]
fn extract_icon(_file_name: &str, ext: &str) -> Option<String> {
    extract_icon_linux(ext)
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
fn extract_icon(_file_name: &str, _ext: &str) -> Option<String> {
    None
}

// ── Windows: SHGetFileInfo ─────────────────────────────────────────────────

/// Windows implementation: `SHGetFileInfo` with `SHGFI_ICON |
/// SHGFI_USEFILEATTRIBUTES` resolves the icon from the extension's file
/// association without requiring the file to exist.
#[cfg(windows)]
fn extract_icon_windows(file_name: &str) -> Option<String> {
    use windows::Win32::UI::Shell::{
        SHGetFileInfoW, SHFILEINFOW, SHGFI_ICON, SHGFI_USEFILEATTRIBUTES,
    };

    // SHGetFileInfoW expects a wide string. A plain file name (no path) is
    // sufficient because SHGFI_USEFILEATTRIBUTES only looks at the extension.
    let wide: Vec<u16> = file_name.encode_utf16().chain(std::iter::once(0)).collect();

    let mut shfi = SHFILEINFOW::default();
    let result = unsafe {
        SHGetFileInfoW(
            windows::core::PCWSTR(wide.as_ptr()),
            windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_NORMAL,
            Some(&mut shfi),
            std::mem::size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON | SHGFI_USEFILEATTRIBUTES,
        )
    };

    if result == 0 || shfi.hIcon.is_invalid() {
        return None;
    }

    let png_bytes = unsafe { hicon_to_png(shfi.hIcon) };
    let _ = unsafe { windows::Win32::UI::WindowsAndMessaging::DestroyIcon(shfi.hIcon) };
    png_bytes
}

/// Convert an `HICON` to a base64 PNG data URL. Extracts the bitmap via
/// `GetIconInfo` + `GetDIBits`, then encodes with the `image` crate.
#[cfg(windows)]
unsafe fn hicon_to_png(icon: windows::Win32::UI::WindowsAndMessaging::HICON) -> Option<String> {
    use windows::Win32::Graphics::Gdi::{
        DeleteObject, GetDC, GetDIBits, GetObjectW, ReleaseDC, BITMAP, BITMAPINFO,
        BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS,
    };
    use windows::Win32::UI::WindowsAndMessaging::{GetIconInfo, ICONINFO};

    let mut icon_info = ICONINFO::default();
    if GetIconInfo(icon, &mut icon_info).is_err() {
        return None;
    }
    // We only need the color bitmap (hbmColor). For monochrome icons
    // hbmColor is null and hbmMask holds the image; we handle the common
    // 32-bit RGBA case which covers virtually all modern file-type icons.
    let color_bmp = if !icon_info.hbmColor.is_invalid() {
        icon_info.hbmColor
    } else {
        icon_info.hbmMask
    };

    // Cleanup helper: free both bitmaps if valid.
    let cleanup = || {
        if !icon_info.hbmColor.is_invalid() {
            let _ = DeleteObject(icon_info.hbmColor.into());
        }
        if !icon_info.hbmMask.is_invalid() {
            let _ = DeleteObject(icon_info.hbmMask.into());
        }
    };

    let mut bmp = BITMAP::default();
    if GetObjectW(
        color_bmp.into(),
        std::mem::size_of::<BITMAP>() as i32,
        Some(&mut bmp as *mut _ as *mut _),
    ) == 0
    {
        cleanup();
        return None;
    }

    let width = bmp.bmWidth as usize;
    let height = bmp.bmHeight as usize;
    if width == 0 || height == 0 {
        cleanup();
        return None;
    }

    // Request 32-bit BGRA pixels (top-down DIB).
    let mut bi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width as i32,
            biHeight: -(height as i32), // negative = top-down
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };

    let mut pixels = vec![0u8; width * height * 4];
    let hdc = GetDC(None);
    let got = GetDIBits(
        hdc,
        color_bmp,
        0,
        height as u32,
        Some(pixels.as_mut_ptr() as *mut _),
        &mut bi,
        DIB_RGB_COLORS,
    );
    let _ = ReleaseDC(None, hdc);
    cleanup();

    if got == 0 {
        return None;
    }

    // BGRA → RGBA
    for chunk in pixels.chunks_exact_mut(4) {
        chunk.swap(0, 2); // B↔R
    }

    encode_rgba_as_png_data_url(width, height, &pixels)
}

// ── macOS: NSWorkspace.iconForFileType ─────────────────────────────────────

/// macOS implementation: `[NSWorkspace.sharedWorkspace iconForFileType:]`
/// returns the system-associated icon for a UTI or extension. We convert
/// the resulting `NSImage` → TIFF → `NSBitmapImageRep` → PNG.
#[cfg(target_os = "macos")]
fn extract_icon_macos(ext: &str) -> Option<String> {
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2_app_kit::{NSBitmapImageFileType, NSBitmapImageRep, NSImage, NSWorkspace};
    use objc2_foundation::{NSDictionary, NSString};

    unsafe {
        // NSWorkspace.sharedWorkspace
        let workspace = NSWorkspace::sharedWorkspace();

        // iconForFileType: takes an extension or UTI string.
        // Deprecated in favor of iconForContentType:, but the replacement
        // requires UTType from objc2-uniform-type-identifiers. The legacy
        // method accepts a bare extension string, which is what we need.
        let ext_ns = NSString::from_str(ext);
        #[allow(deprecated)]
        let image: Retained<NSImage> = workspace.iconForFileType(&ext_ns);

        // NSImage → TIFF data
        let tiff_data = image.TIFFRepresentation()?;

        // TIFF → NSBitmapImageRep
        let bitmap_rep = NSBitmapImageRep::imageRepWithData(&tiff_data)?;

        // NSBitmapImageRep → PNG data. The properties dict can be empty —
        // NSBitmapImageRepPropertyKey is a type alias for NSString, so we
        // use NSString as the key type.
        let empty_props = NSDictionary::<NSString, AnyObject>::new();
        let png_data = bitmap_rep
            .representationUsingType_properties(NSBitmapImageFileType::PNG, &empty_props)?;

        // NSData → bytes → base64. as_bytes_unchecked is the correct way to
        // access NSData bytes in objc2-foundation 0.3.x — the older bytes()
        // method returning a raw pointer was removed.
        let bytes = png_data.as_bytes_unchecked();
        let b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, bytes);
        Some(format!("data:image/png;base64,{b64}"))
    }
}

// ── Linux: freedesktop icon theme ──────────────────────────────────────────

/// Linux implementation: resolves the extension to a freedesktop icon name
/// (e.g. `video-mp4` → `video-x-generic`), then looks up the icon in the
/// current icon theme via the `freedesktop` crate. The icon file (PNG/ICO)
/// is read and re-encoded as PNG.
#[cfg(target_os = "linux")]
fn extract_icon_linux(ext: &str) -> Option<String> {
    // Map extension → freedesktop icon name.
    let icon_name = icon_name_for_ext(ext)?;

    // Look up the icon in the current theme.
    let icon_path = freedesktop::get_icon(&icon_name)?;

    // Read the icon file and encode as PNG data URL.
    let file_bytes = std::fs::read(&icon_path).ok()?;

    // Try to decode with the `image` crate (handles PNG, ICO, XPM, etc.).
    let img = image::load_from_memory(&file_bytes).ok()?;
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    encode_rgba_as_png_data_url(w as usize, h as usize, rgba.as_raw())
}

/// Map a file extension to a freedesktop icon name. Uses the generic
/// category icon (e.g. `video-x-generic`, `x-office-document`) since
/// freedesktop icon themes ship these as standard entries. The
/// `freedesktop::get_icon` call resolves the theme fallback chain.
#[cfg(target_os = "linux")]
fn icon_name_for_ext(ext: &str) -> Option<String> {
    // freedesktop icon naming convention uses "category-x-generic" for
    // generic file-type icons. Some categories (archives, executables) use
    // MIME-style names directly.
    Some(category_prefix(ext)?.to_string())
}

#[cfg(target_os = "linux")]
fn category_prefix(ext: &str) -> Option<&'static str> {
    Some(match ext {
        "mp4" | "mkv" | "avi" | "mov" | "webm" | "flv" | "wmv" | "m4v" | "mpg" | "mpeg" | "ts" => {
            "video-x-generic"
        }
        "mp3" | "wav" | "flac" | "aac" | "ogg" | "opus" | "m4a" | "wma" => "audio-x-generic",
        "jpg" | "jpeg" | "png" | "gif" | "webp" | "bmp" | "svg" | "tiff" | "ico" => {
            "image-x-generic"
        }
        "pdf" => "x-office-document",
        "doc" | "docx" | "odt" | "rtf" | "txt" | "md" => "x-office-document",
        "xls" | "xlsx" | "ods" | "csv" | "tsv" => "x-office-spreadsheet",
        "ppt" | "pptx" | "odp" => "x-office-presentation",
        "zip" | "rar" | "7z" | "tar" | "gz" | "bz2" | "xz" => "package-x-generic",
        "exe" | "msi" | "deb" | "rpm" | "apk" | "appimage" => "application-x-executable",
        "torrent" => "application-x-bittorrent",
        "iso" | "img" => "application-x-cd-image",
        "js" | "py" | "rs" | "go" | "java" | "c" | "cpp" | "h" | "json" | "xml" | "yaml"
        | "html" | "css" => "text-x-script",
        _ => "text-x-generic",
    })
}

// ── Shared helpers ─────────────────────────────────────────────────────────

/// Encode raw RGBA pixels as a PNG base64 data URL.
#[cfg(any(windows, target_os = "linux"))]
fn encode_rgba_as_png_data_url(width: usize, height: usize, rgba: &[u8]) -> Option<String> {
    let img = image::RgbaImage::from_raw(width as u32, height as u32, rgba.to_vec())?;
    let mut png = Vec::with_capacity(rgba.len());
    image::DynamicImage::ImageRgba8(img)
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .ok()?;
    let b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &png);
    Some(format!("data:image/png;base64,{b64}"))
}

/// Very small extension→MIME hint map. Not exhaustive — just the common
/// download categories so the frontend has a fallback label.
fn mime_for_ext(ext: &str) -> Option<&'static str> {
    Some(match ext {
        "mp4" | "mkv" | "avi" | "mov" | "webm" | "flv" | "wmv" | "m4v" => "video",
        "mp3" | "wav" | "flac" | "aac" | "ogg" | "opus" | "m4a" => "audio",
        "jpg" | "jpeg" | "png" | "gif" | "webp" | "bmp" | "svg" | "ico" => "image",
        "zip" | "rar" | "7z" | "tar" | "gz" | "bz2" | "xz" => "archive",
        "pdf" => "application/pdf",
        "exe" | "msi" | "apk" | "deb" | "rpm" | "dmg" => "application",
        "torrent" => "application/x-bittorrent",
        "iso" | "img" => "application/x-iso9660-image",
        "txt" | "md" => "text/plain",
        "doc" | "docx" => "application/msword",
        "xls" | "xlsx" | "csv" => "application/vnd.ms-excel",
        _ => return None,
    })
}
