//! macOS. Copy and paste use `pbcopy` and `pbpaste`, which every Mac has.
//!
//! The program isn't installed from inside the app: on a Mac it is a
//! Copperview.app that `make-macos-app.sh` builds, to drag into Applications.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

pub const CAN_INSTALL: bool = false;

/// Where the program keeps its small settings, such as the boards last opened.
pub fn config_dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from)?;
    Some(home.join("Library/Application Support/copperview"))
}

fn tool(name: &str) -> Command {
    let mut command = Command::new(name);
    // Apps opened from the Finder have no language set, and the tools would
    // then read and write text as Mac Roman instead of UTF-8.
    command.env("LC_CTYPE", "UTF-8");
    command
}

pub fn clipboard_copy(text: &str) -> bool {
    let child = tool("pbcopy").stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).spawn();
    let Ok(mut child) = child else { return false };
    let wrote = child.stdin.take().is_some_and(|mut pipe| pipe.write_all(text.as_bytes()).is_ok());
    child.wait().is_ok_and(|status| status.success()) && wrote
}

pub fn clipboard_paste() -> Option<String> {
    let out = tool("pbpaste").stderr(Stdio::null()).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).replace("\r\n", "\n"))
}

/// Points the app menu's "About Copperview" at the program's own About window.
/// The menu belongs to the window library and may not exist yet on the first
/// frames, so this is tried each frame until it gives true.
pub fn hook_about_menu(ctx: &eframe::egui::Context) -> bool {
    #[cfg(target_os = "macos")]
    {
        about_menu::hook(ctx)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = ctx;
        true
    }
}

#[cfg(target_os = "macos")]
mod about_menu {
    use objc2::rc::Retained;
    use objc2::runtime::{AnyObject, NSObject};
    use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
    use objc2_app_kit::NSApplication;
    use std::sync::OnceLock;

    static CTX: OnceLock<eframe::egui::Context> = OnceLock::new();

    define_class!(
        #[unsafe(super(NSObject))]
        #[thread_kind = MainThreadOnly]
        #[name = "CopperviewAboutTarget"]
        struct AboutTarget;

        impl AboutTarget {
            #[unsafe(method(showAbout:))]
            fn show_about(&self, _sender: Option<&AnyObject>) {
                crate::platform::request_about();
                if let Some(ctx) = CTX.get() {
                    ctx.request_repaint();
                }
            }
        }
    );

    impl AboutTarget {
        fn new(mtm: MainThreadMarker) -> Retained<Self> {
            let this = Self::alloc(mtm).set_ivars(());
            unsafe { msg_send![super(this), init] }
        }
    }

    pub fn hook(ctx: &eframe::egui::Context) -> bool {
        let Some(mtm) = MainThreadMarker::new() else { return false };
        let app = NSApplication::sharedApplication(mtm);
        // The first item of the first menu is "About Copperview".
        let Some(about) = app.mainMenu().and_then(|menu| menu.itemAtIndex(0)).and_then(|item| item.submenu()).and_then(|menu| menu.itemAtIndex(0))
        else {
            return false;
        };
        let _ = CTX.set(ctx.clone());
        let target = AboutTarget::new(mtm);
        unsafe {
            about.setTarget(Some(&target));
            about.setAction(Some(sel!(showAbout:)));
        }
        // A menu item doesn't keep its target alive, so this one stays for good.
        std::mem::forget(target);
        true
    }
}

pub fn clipboard_missing_hint() -> &'static str {
    "Copying didn't work: the pbcopy tool wasn't found."
}

pub fn install() -> Result<(), String> {
    Err("On a Mac, build Copperview.app with make-macos-app.sh and drag it into Applications.".to_string())
}

pub fn uninstall() -> Result<(), String> {
    Err("On a Mac, remove Copperview by dragging Copperview.app out of Applications.".to_string())
}

pub fn is_current() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_go_where_a_mac_keeps_them() {
        let dir = config_dir();
        if std::env::var_os("HOME").is_some() {
            assert!(dir.unwrap().ends_with("Library/Application Support/copperview"));
        }
    }

    #[test]
    fn the_tools_are_told_to_use_utf8() {
        let command = tool("pbcopy");
        let set: Vec<_> = command.get_envs().collect();
        assert!(set.iter().any(|(k, v)| *k == "LC_CTYPE" && v.is_some_and(|v| v == "UTF-8")));
    }
}
