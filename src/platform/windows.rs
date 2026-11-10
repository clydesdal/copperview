//! Windows. Copy and paste go through PowerShell, which every Windows has.
//!
//! The program isn't installed from inside the app: the Windows installer
//! (make-windows-installer.ps1) does that, or copperview.exe runs from anywhere.

use std::path::PathBuf;
use std::process::Command;

pub const CAN_INSTALL: bool = false;

/// Where the program keeps its small settings, such as the boards last opened.
pub fn config_dir() -> Option<PathBuf> {
    let base = std::env::var_os("APPDATA").filter(|v| !v.is_empty()).map(PathBuf::from)?;
    Some(base.join("copperview"))
}

/// A PowerShell command that runs without a console window flashing up.
fn powershell(script: &str) -> Command {
    let mut command = Command::new("powershell");
    command.args(["-NoProfile", "-NonInteractive", "-Command", script]);
    hide_window(&mut command);
    command
}

#[cfg(windows)]
fn hide_window(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn hide_window(_command: &mut Command) {}

/// The text goes in through an environment variable, so that characters such
/// as × reach the clipboard intact whatever the console's code page is.
fn copy_command(text: &str) -> Command {
    let mut command = powershell("Set-Clipboard -Value $env:COPPERVIEW_TEXT");
    command.env("COPPERVIEW_TEXT", text);
    command
}

fn paste_command() -> Command {
    powershell("[Console]::OutputEncoding = [System.Text.Encoding]::UTF8; Get-Clipboard -Raw")
}

pub fn clipboard_copy(text: &str) -> bool {
    copy_command(text).output().is_ok_and(|out| out.status.success())
}

pub fn clipboard_paste() -> Option<String> {
    let out = paste_command().output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).replace("\r\n", "\n"))
}

/// Nothing to hook here: there's no system menu with its own About item. Says it's done.
pub fn hook_about_menu(_ctx: &eframe::egui::Context) -> bool {
    true
}

pub fn clipboard_missing_hint() -> &'static str {
    "Copying didn't work: PowerShell couldn't be run."
}

pub fn install() -> Result<(), String> {
    Err("On Windows, run the Copperview setup program to install, or keep copperview.exe wherever you like.".to_string())
}

pub fn uninstall() -> Result<(), String> {
    Err("On Windows, remove Copperview in Settings > Apps, or just delete copperview.exe if you didn't install it.".to_string())
}

pub fn is_current() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copying_hands_the_text_over_untouched() {
        let command = copy_command("Pad  1.50 × 0.45 mm");
        let set: Vec<_> = command.get_envs().collect();
        assert!(set.iter().any(|(k, v)| *k == "COPPERVIEW_TEXT" && v.is_some_and(|v| v == "Pad  1.50 × 0.45 mm")));
        assert_eq!(command.get_program(), "powershell");
    }

    #[test]
    fn pasting_asks_for_utf8_text() {
        let args: Vec<_> = paste_command().get_args().map(|a| a.to_string_lossy().into_owned()).collect();
        assert!(args.last().is_some_and(|a| a.contains("UTF8") && a.contains("Get-Clipboard")));
    }
}
