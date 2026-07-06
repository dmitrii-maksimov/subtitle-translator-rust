//! External-tool discovery and subprocess helpers, ported from `utils.py`.
//!
//! ffmpeg/ffprobe stay external binaries; we only locate and invoke them.

use std::path::PathBuf;
use std::process::Command;

/// Directory containing the current executable (falls back to cwd).
pub fn base_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
}

#[cfg(target_os = "macos")]
const MACOS_EXTRA_PATHS: &[&str] = &["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin", "/bin"];

fn exe_name(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    }
}

/// Return the absolute path to `name` (e.g. "ffmpeg").
///
/// Search order mirrors the Python version:
///   1. a binary placed next to our executable (bundled build);
///   2. a PATH scan, augmented on macOS with common Homebrew dirs.
pub fn find_tool(name: &str) -> Result<PathBuf, String> {
    let exe = exe_name(name);

    let local = base_dir().join(&exe);
    if is_executable_file(&local) {
        return Ok(local);
    }

    let mut search_paths: Vec<PathBuf> = Vec::new();
    if let Some(path_var) = std::env::var_os("PATH") {
        search_paths.extend(std::env::split_paths(&path_var));
    }
    #[cfg(target_os = "macos")]
    {
        for extra in MACOS_EXTRA_PATHS {
            let p = PathBuf::from(extra);
            if !search_paths.contains(&p) {
                search_paths.push(p);
            }
        }
    }

    for dir in search_paths {
        let candidate = dir.join(&exe);
        if is_executable_file(&candidate) {
            return Ok(candidate);
        }
    }

    Err(if cfg!(target_os = "macos") {
        format!("{name} not found.\nInstall via Homebrew:  brew install ffmpeg")
    } else {
        format!("{name} not found.\nInstall ffmpeg and make sure it is in PATH.")
    })
}

fn is_executable_file(path: &PathBuf) -> bool {
    if !path.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(md) = path.metadata() {
            return md.permissions().mode() & 0o111 != 0;
        }
        false
    }
    #[cfg(not(unix))]
    {
        true
    }
}

/// Build a `Command` for the given tool, hiding the console window on Windows.
pub fn tool_command(path: &PathBuf) -> Command {
    let cmd = Command::new(path);
    apply_no_window(cmd)
}

#[cfg(windows)]
fn apply_no_window(mut cmd: Command) -> Command {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd
}

#[cfg(not(windows))]
fn apply_no_window(cmd: Command) -> Command {
    cmd
}

/// True if both ffmpeg and ffprobe are discoverable and respond to `-version`.
pub fn check_ffmpeg_available() -> bool {
    let (ffmpeg, ffprobe) = match (find_tool("ffmpeg"), find_tool("ffprobe")) {
        (Ok(a), Ok(b)) => (a, b),
        _ => return false,
    };
    let ok = |p: &PathBuf| {
        tool_command(p)
            .arg("-version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    };
    ok(&ffmpeg) && ok(&ffprobe)
}
