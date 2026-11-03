//! Copy and paste through the desktop's own clipboard tools. The window
//! library's built-in clipboard adds about a tenth to the program's size.

use std::io::Write;
use std::process::{Command, Stdio};

fn on_wayland() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_some()
}

/// Puts text on the clipboard. Gives false when no tool could be found.
pub fn copy(text: &str) -> bool {
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
pub fn paste() -> Option<String> {
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
        assert!(copy("Pad  1.50 × 0.45 mm"));
        for _ in 0..200 {
            if std::fs::metadata(&held).is_ok_and(|m| m.len() > 0) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(paste().as_deref(), Some("Pad  1.50 × 0.45 mm"));
        unsafe { std::env::set_var("PATH", path) };
    }
}
