use std::{env, path::PathBuf, process::Command};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

const CREATE_NO_WINDOW: u32 = 0x08000000;

fn executable_dir() -> Option<PathBuf> {
    env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(PathBuf::from))
}

pub fn hide_child_console(command: &mut Command) {
    #[cfg(windows)]
    {
        command.creation_flags(CREATE_NO_WINDOW);
    }
}

fn dev_vendor_path(parts: &[&str]) -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

    for part in parts {
        path.push(part);
    }

    path
}

fn bundled_or_dev_path(
    bundled_name: &str,
    path_command: &str,
    dev_vendor_parts: &[&str],
) -> PathBuf {
    let bundled = executable_dir().map(|directory| directory.join(bundled_name));

    if let Some(path) = &bundled {
        if path.is_file() {
            return path.clone();
        }
    }

    // Packaged release builds must use the dependency beside the EXE.
    // Do not silently fall back to a developer copy or system PATH.
    if !cfg!(debug_assertions) {
        return bundled.unwrap_or_else(|| PathBuf::from(bundled_name));
    }

    // Debug/source builds keep convenient development fallbacks.
    let dev_vendor = dev_vendor_path(dev_vendor_parts);

    if dev_vendor.is_file() {
        return dev_vendor;
    }

    PathBuf::from(path_command)
}

pub fn ffmpeg_path() -> PathBuf {
    bundled_or_dev_path("ffmpeg.exe", "ffmpeg", &["vendor", "ffmpeg", "ffmpeg.exe"])
}

pub fn ffprobe_path() -> PathBuf {
    bundled_or_dev_path(
        "ffprobe.exe",
        "ffprobe",
        &["vendor", "ffmpeg", "ffprobe.exe"],
    )
}

pub fn libmpv_path() -> Result<PathBuf, String> {
    let bundled = executable_dir().map(|directory| directory.join("libmpv-2.dll"));

    if let Some(path) = &bundled {
        if path.is_file() {
            return Ok(path.clone());
        }
    }

    // A packaged release must not silently reach back into the developer repo.
    if !cfg!(debug_assertions) {
        let expected = bundled
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "libmpv-2.dll beside Simple MKV Player.exe".to_string());

        return Err(format!("libmpv-2.dll was not found at {expected}."));
    }

    let dev_vendor = dev_vendor_path(&["vendor", "mpv", "libmpv-2.dll"]);

    if dev_vendor.is_file() {
        return Ok(dev_vendor);
    }

    Err("libmpv-2.dll was not found beside Simple MKV Player.exe or in vendor\\mpv.".to_string())
}
