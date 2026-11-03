//! Linux and other Unix desktops.
//!
//! Installing: the program goes in ~/Applications beside the other apps, with
//! a launcher entry, an icon and a `copperview` terminal command. Nothing here
//! runs unless the person asks, and it stays inside their home.
//!
//! Copy and paste go through the desktop's own clipboard tools: the window
//! library's built-in clipboard adds about a tenth to the program's size.

use crate::icons;
use std::io::Write;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// The program can put itself in the launcher from inside the app.
pub const CAN_INSTALL: bool = true;

const NAME: &str = "copperview";

fn home() -> Result<PathBuf, String> {
    std::env::var_os("HOME").map(PathBuf::from).ok_or("The home folder couldn't be found.".to_string())
}

/// Where the program keeps its small settings, such as the boards last opened.
pub fn config_dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| Path::new(&h).join(".config")))?;
    Some(base.join("copperview"))
}

fn program_path() -> Result<PathBuf, String> {
    Ok(home()?.join("Applications").join(NAME))
}

fn command_path() -> Result<PathBuf, String> {
    Ok(home()?.join(".local/bin").join(NAME))
}

fn share_dir() -> Result<PathBuf, String> {
    match std::env::var_os("XDG_DATA_HOME").filter(|v| !v.is_empty()) {
        Some(dir) => Ok(PathBuf::from(dir)),
        None => Ok(home()?.join(".local/share")),
    }
}

fn desktop_file() -> Result<PathBuf, String> {
    Ok(share_dir()?.join("applications").join(format!("{NAME}.desktop")))
}

fn icon_file(size: u32) -> Result<PathBuf, String> {
    Ok(share_dir()?.join(format!("icons/hicolor/{size}x{size}/apps/{NAME}.png")))
}

fn scalable_icon() -> Result<PathBuf, String> {
    Ok(share_dir()?.join(format!("icons/hicolor/scalable/apps/{NAME}.svg")))
}

fn make_room(path: &Path) -> Result<(), String> {
    match path.parent() {
        Some(dir) => std::fs::create_dir_all(dir).map_err(|e| format!("{} couldn't be made: {e}", dir.display())),
        None => Ok(()),
    }
}

/// True when this very program is the one in the Applications folder.
pub fn is_current() -> bool {
    let Ok(installed) = program_path() else { return false };
    let size = |p: &Path| std::fs::metadata(p).map(|m| m.len()).ok();
    std::env::current_exe()
        .is_ok_and(|exe| exe == installed || (size(&installed).is_some() && size(&exe) == size(&installed)))
}

fn write_pictures() -> Result<(), String> {
    for (size, bytes) in icons::PICTURES {
        let path = icon_file(size)?;
        make_room(&path)?;
        std::fs::write(&path, bytes).map_err(|e| format!("The icon couldn't be saved: {e}"))?;
    }
    let path = scalable_icon()?;
    make_room(&path)?;
    std::fs::write(&path, icons::SCALABLE).map_err(|e| format!("The icon couldn't be saved: {e}"))
}

/// Lets the launcher know, if the tools for that are around. It finds new
/// entries by itself in time, so nothing's wrong if they aren't.
fn refresh_launcher() {
    let Ok(share) = share_dir() else { return };
    let run = |tool: &str, args: &[&str], dir: PathBuf| {
        let _ = Command::new(tool).args(args).arg(dir).stdout(Stdio::null()).stderr(Stdio::null()).status();
    };
    run("update-desktop-database", &[], share.join("applications"));
    run("gtk-update-icon-cache", &["-q", "-t", "-f"], share.join("icons/hicolor"));
}

pub fn install() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| format!("The program couldn't find itself: {e}"))?;
    let program = program_path()?;
    make_room(&program)?;
    if exe != program {
        // Written beside it first, so a copy that's running isn't disturbed.
        let fresh = program.with_extension("new");
        std::fs::copy(&exe, &fresh).map_err(|e| format!("The program couldn't be copied: {e}"))?;
        std::fs::set_permissions(&fresh, std::fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
        std::fs::rename(&fresh, &program).map_err(|e| format!("The program couldn't be put in place: {e}"))?;
    }

    let command = command_path()?;
    make_room(&command)?;
    // Only ever replaces a link, so it can't overwrite someone's own file.
    if command.is_symlink() {
        std::fs::remove_file(&command).map_err(|e| e.to_string())?;
    }
    if !command.exists() {
        symlink(&program, &command).map_err(|e| format!("The terminal command couldn't be made: {e}"))?;
    }

    write_pictures()?;
    let desktop = desktop_file()?;
    make_room(&desktop)?;
    let entry = format!(
        "[Desktop Entry]\nType=Application\nName=Copperview\n\
         Comment=Look at PCB Gerber files, measure them, compare two sets\n\
         Exec=\"{}\" %F\nIcon={NAME}\nTerminal=false\nCategories=Development;Electronics;Engineering;\n\
         MimeType=application/vnd.gerber;application/x-gerber;application/x-excellon;\n\
         Keywords=gerber;pcb;altium;drill;board;\nStartupWMClass={NAME}\n",
        program.display()
    );
    std::fs::write(&desktop, entry).map_err(|e| format!("The launcher entry couldn't be made: {e}"))?;
    refresh_launcher();
    Ok(())
}

pub fn uninstall() -> Result<(), String> {
    let program = program_path()?;
    let command = command_path()?;
    let mut files = vec![program.clone(), desktop_file()?, scalable_icon()?];
    for (size, _) in icons::PICTURES {
        files.push(icon_file(size)?);
    }
    if command.is_symlink() && std::fs::read_link(&command).is_ok_and(|to| to == program) {
        files.push(command);
    }
    for file in files {
        match std::fs::remove_file(&file) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                return Err(format!("{} couldn't be removed: {e}", file.display()));
            }
            _ => {}
        }
    }
    refresh_launcher();
    Ok(())
}

fn on_wayland() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_some()
}

/// Puts text on the clipboard. Gives false when no tool could be found.
pub fn clipboard_copy(text: &str) -> bool {
    let tools: [(&str, &[&str]); 3] =
        [("wl-copy", &[]), ("xclip", &["-selection", "clipboard"]), ("xsel", &["-ib"])];
    let start = if on_wayland() { 0 } else { 1 };
    for (tool, args) in tools[start..].iter().chain(&tools[..start]) {
        let child = Command::new(tool)
            .args(*args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        let Ok(mut child) = child else { continue };
        let wrote = child.stdin.take().is_some_and(|mut pipe| pipe.write_all(text.as_bytes()).is_ok());
        // Clipboard tools can stay running to hold the text: don't leave one unattended.
        std::thread::spawn(move || child.wait());
        if wrote {
            return true;
        }
    }
    false
}

/// What's on the clipboard, if it's text.
pub fn clipboard_paste() -> Option<String> {
    let tools: [(&str, &[&str]); 3] =
        [("wl-paste", &["-n"]), ("xclip", &["-o", "-selection", "clipboard"]), ("xsel", &["-ob"])];
    let start = if on_wayland() { 0 } else { 1 };
    for (tool, args) in tools[start..].iter().chain(&tools[..start]) {
        if let Ok(out) = Command::new(tool).args(*args).stderr(Stdio::null()).output()
            && out.status.success()
        {
            return Some(String::from_utf8_lossy(&out.stdout).replace("\r\n", "\n"));
        }
    }
    None
}

/// What to tell the person when no clipboard tool could be found.
/// Nothing to hook here: there's no system menu with its own About item. Says it's done.
pub fn hook_about_menu(_ctx: &eframe::egui::Context) -> bool {
    true
}

pub fn clipboard_missing_hint() -> &'static str {
    "Copying needs wl-clipboard or xclip installed."
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    /// Stand-in clipboard tools, so the test leaves the real clipboard alone.
    #[test]
    fn copy_and_paste_reach_the_desktop_tools() {
        let dir = std::env::temp_dir().join(format!("copperview-clip-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let held = dir.join("held");
        let copy_tool = format!("cat > '{}'", held.display());
        let paste_tool = format!("cat '{}'", held.display());
        for (name, body) in [("wl-copy", copy_tool), ("wl-paste", paste_tool)] {
            let tool = dir.join(name);
            std::fs::write(&tool, format!("#!/bin/sh\n{body}\n")).unwrap();
            std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let path = std::env::var("PATH").unwrap_or_default();
        // Nothing else in the tests starts programs.
        unsafe {
            std::env::set_var("PATH", format!("{}:{path}", dir.display()));
            std::env::set_var("WAYLAND_DISPLAY", "test");
        }
        assert!(clipboard_copy("Pad  1.50 × 0.45 mm"));
        for _ in 0..200 {
            if std::fs::metadata(&held).is_ok_and(|m| m.len() > 0) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(clipboard_paste().as_deref(), Some("Pad  1.50 × 0.45 mm"));
        unsafe { std::env::set_var("PATH", path) };
    }
}
