//! The few things that need to ask the operating system directly.

/// Tells Windows the app can give up physical memory it isn't touching. Pages come back
/// on demand, so going still means almost nothing stays resident.
#[cfg(windows)]
pub fn trim_memory() {
    use windows_sys::Win32::System::ProcessStatus::K32EmptyWorkingSet;
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    // SAFETY: both calls only touch the current process and take no borrowed data.
    unsafe {
        K32EmptyWorkingSet(GetCurrentProcess());
    }
}

#[cfg(not(windows))]
pub fn trim_memory() {}

/// Whether the person has asked the system for fewer animations.
#[cfg(windows)]
pub fn prefers_reduced_motion() -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{SPI_GETCLIENTAREAANIMATION, SystemParametersInfoW};
    let mut enabled: i32 = 1;
    // SAFETY: the out-pointer is valid for the duration of the call.
    let ok = unsafe { SystemParametersInfoW(SPI_GETCLIENTAREAANIMATION, 0, (&raw mut enabled).cast(), 0) };
    ok != 0 && enabled == 0
}

#[cfg(not(windows))]
pub fn prefers_reduced_motion() -> bool {
    false
}

/// A disk's name and how full it is.
#[derive(Clone, Debug, Default)]
pub struct DriveInfo {
    /// How it's shown: "Windows (C:)" or "Local Disk (C:)".
    pub display: String,
    pub free: u64,
    pub total: u64,
}

/// The letters of every disk Windows knows about. It never touches the disks themselves,
/// so a sleeping or disconnected drive can't stall it.
#[cfg(windows)]
pub fn drive_letters() -> Vec<char> {
    use windows_sys::Win32::Storage::FileSystem::GetLogicalDrives;
    // SAFETY: takes no arguments and only returns a bitmask.
    let mask = unsafe { GetLogicalDrives() };
    (0..26).filter(|i| mask & (1 << i) != 0).map(|i| (b'A' + i as u8) as char).collect()
}

/// Reads a disk's name and free space. This can wait on the disk, so it runs off the
/// interface thread.
#[cfg(windows)]
pub fn drive_info(name: &str) -> Option<DriveInfo> {
    use windows_sys::Win32::Storage::FileSystem::{
        GetDiskFreeSpaceExW, GetDriveTypeW, GetVolumeInformationW,
    };
    let root: Vec<u16> = format!("{name}\\").encode_utf16().chain(std::iter::once(0)).collect();
    let mut label = [0u16; 261];
    let (mut free, mut total, mut any) = (0u64, 0u64, 0u64);
    // SAFETY: `root` is NUL-terminated, the buffers outlive the calls, and every other
    // pointer is either valid for the call or null where the API allows it.
    let kind = unsafe {
        let has_label = GetVolumeInformationW(
            root.as_ptr(),
            label.as_mut_ptr(),
            label.len() as u32,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            0,
        );
        if has_label == 0 {
            label[0] = 0;
        }
        GetDiskFreeSpaceExW(root.as_ptr(), &raw mut any, &raw mut total, &raw mut free);
        GetDriveTypeW(root.as_ptr())
    };
    let end = label.iter().position(|c| *c == 0).unwrap_or(0);
    let label = String::from_utf16_lossy(&label[..end]);
    let fallback = match kind {
        2 => "USB Drive",
        4 => "Network Drive",
        5 => "CD Drive",
        _ => "Local Disk",
    };
    let shown = if label.trim().is_empty() { fallback } else { label.trim() };
    Some(DriveInfo { display: format!("{shown} ({name})"), free, total })
}

#[cfg(not(windows))]
pub fn drive_info(_name: &str) -> Option<DriveInfo> {
    None
}

/// What Windows itself would show as this file's thumbnail: a video's frame, a PDF's first
/// page, an Office document's cover. Only real thumbnails, never generic icons.
#[cfg(windows)]
pub fn thumbnail(path: &std::path::Path, side: i32) -> Option<(u32, u32, Vec<u8>)> {
    use windows::Win32::Foundation::SIZE;
    use windows::Win32::Graphics::Gdi::{
        BI_RGB, BITMAP, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, DIB_RGB_COLORS, DeleteDC, DeleteObject,
        GetDIBits, GetObjectW, HGDIOBJ,
    };
    use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
    use windows::Win32::UI::Shell::{
        IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_BIGGERSIZEOK, SIIGBF_THUMBNAILONLY,
    };
    use windows::core::HSTRING;
    // SAFETY: every handle is checked and released here, and the output buffer is sized from
    // the bitmap's own dimensions.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let wide = HSTRING::from(path.as_os_str());
        let factory: IShellItemImageFactory = SHCreateItemFromParsingName(&wide, None).ok()?;
        let bitmap = factory.GetImage(SIZE { cx: side, cy: side }, SIIGBF_THUMBNAILONLY | SIIGBF_BIGGERSIZEOK).ok()?;
        let mut info = BITMAP::default();
        let wrote = GetObjectW(HGDIOBJ(bitmap.0), size_of::<BITMAP>() as i32, Some((&raw mut info).cast()));
        let (w, h) = (info.bmWidth, info.bmHeight.abs());
        let mut pixels = vec![0u8; (w.max(0) * h.max(0) * 4) as usize];
        let mut request = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h, // Negative asks for rows from the top down.
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let dc = CreateCompatibleDC(None);
        let rows = if wrote > 0 && w > 0 && h > 0 {
            GetDIBits(dc, bitmap, 0, h as u32, Some(pixels.as_mut_ptr().cast()), &raw mut request, DIB_RGB_COLORS)
        } else {
            0
        };
        let _ = DeleteDC(dc);
        let _ = DeleteObject(HGDIOBJ(bitmap.0));
        if rows == 0 {
            return None;
        }
        // Many thumbnails carry no alpha at all, which would make them invisible.
        let opaque = pixels.chunks_exact(4).all(|p| p[3] == 0);
        for p in pixels.chunks_exact_mut(4) {
            p.swap(0, 2);
            if opaque {
                p[3] = 255;
            }
        }
        Some((w as u32, h as u32, pixels))
    }
}

#[cfg(not(windows))]
pub fn thumbnail(_path: &std::path::Path, _side: i32) -> Option<(u32, u32, Vec<u8>)> {
    None
}

/// The folders people go to most, wherever this computer really keeps them. On a OneDrive
/// setup, for instance, Documents isn't where it looks like it should be.
#[cfg(windows)]
pub fn quick_places() -> Vec<(&'static str, std::path::PathBuf)> {
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::Win32::UI::Shell::{
        FOLDERID_Desktop, FOLDERID_Documents, FOLDERID_Downloads, FOLDERID_Music, FOLDERID_Pictures, FOLDERID_Videos,
        KF_FLAG_DEFAULT, SHGetKnownFolderPath,
    };
    let places = [
        ("Desktop", FOLDERID_Desktop),
        ("Documents", FOLDERID_Documents),
        ("Downloads", FOLDERID_Downloads),
        ("Pictures", FOLDERID_Pictures),
        ("Music", FOLDERID_Music),
        ("Videos", FOLDERID_Videos),
    ];
    let mut out = Vec::new();
    for (name, id) in places {
        // SAFETY: the returned string is copied and then freed exactly once.
        unsafe {
            if let Ok(raw) = SHGetKnownFolderPath(&id, KF_FLAG_DEFAULT, None) {
                if let Ok(text) = raw.to_string() {
                    out.push((name, std::path::PathBuf::from(text)));
                }
                CoTaskMemFree(Some(raw.0 as *const _));
            }
        }
    }
    out
}

#[cfg(not(windows))]
pub fn quick_places() -> Vec<(&'static str, std::path::PathBuf)> {
    let home = std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default());
    ["Desktop", "Documents", "Downloads", "Pictures", "Music", "Videos"].map(|n| (n, home.join(n))).into_iter().collect()
}

/// Asks Windows which app to open this file with.
pub fn open_with(path: &std::path::Path) -> std::io::Result<()> {
    if cfg!(windows) {
        std::process::Command::new("rundll32.exe").arg("shell32.dll,OpenAs_RunDLL").arg(path).spawn().map(|_| ())
    } else {
        opener::open(path).map_err(std::io::Error::other)
    }
}

/// Opens a terminal in this folder.
pub fn open_terminal(dir: &std::path::Path) -> std::io::Result<()> {
    use std::process::Command;
    if cfg!(windows) {
        // Windows Terminal if it's there, otherwise the classic console.
        if Command::new("wt.exe").arg("-d").arg(dir).spawn().is_ok() {
            return Ok(());
        }
        return Command::new("cmd.exe").args(["/c", "start", "cmd.exe"]).current_dir(dir).spawn().map(|_| ());
    }
    if cfg!(target_os = "macos") {
        return Command::new("open").args(["-a", "Terminal"]).arg(dir).spawn().map(|_| ());
    }
    Command::new("x-terminal-emulator").current_dir(dir).spawn().map(|_| ())
}

/// Says something went wrong in a way that can be seen, since the app has no console to print to.
#[cfg(windows)]
pub fn tell_user(message: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};
    let wide = |text: &str| text.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
    // SAFETY: both strings are NUL-terminated and outlive the call.
    unsafe {
        MessageBoxW(std::ptr::null_mut(), wide(message).as_ptr(), wide("FileTree").as_ptr(), MB_OK | MB_ICONERROR);
    }
}

#[cfg(not(windows))]
pub fn tell_user(message: &str) {
    eprintln!("{message}");
}
