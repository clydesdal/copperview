//! Copperview: look at PCB Gerber files, measure anything you click, and
//! compare two folders or zips.

// On Windows, a window program rather than a console one, so no black box
// opens beside it. Typed into a console, it still prints there (see main).
#![cfg_attr(windows, windows_subsystem = "windows")]

mod altium;
mod app;
mod board;
mod board3d;
mod compare;
mod excellon;
mod geom;
mod gerber;
mod gpu;
mod gpu3d;
mod icons;
mod markup;
mod panels;
mod pinch;
mod platform;
mod play;
mod samples;
mod trace;
mod theme;
mod view;
mod view3d;

use app::Launch;
use eframe::egui;
use geom::{P, p};
use std::path::PathBuf;

const HELP: &str = "\
Copperview: look at Gerber files, measure them, compare two sets.

  copperview                    open the window
  copperview FOLDER_OR_ZIP      open one set
  copperview A B                compare two sets (folders, zips or one of each)
  copperview install            add to the Applications folder and app launcher
  copperview uninstall          take it out again

Options:
  --view a|b|side|stack|diff|swipe
                                start in this view
  --mix light|ink               start with this color mixing
  --unit mil|in                 start in these units
  --all-layers                  start with every layer showing
  --flip                        start seen from the bottom
  --version                     print the version
  --no-details                  start with the details panel folded away
  --3d                          start in the 3D view
  --zoom X,Y,WIDTH              start looking at this spot, WIDTH mm across
  --pick X,Y                    start with the feature at this spot measured
  --ruler X1,Y1,X2,Y2           start with a ruler between two spots
  --screenshot FILE.png         draw 120 frames, save the picture, and exit
  --play STEPS                  with --screenshot: act out mouse and key steps,
                                such as 'click 600 400; key m; scroll 600 400 200'
  --write-sample DIR            write the sample board as real files, A and B
";

fn numbers(text: &str, count: usize, name: &str) -> Result<Vec<f64>, String> {
    let values: Vec<f64> = text.split(',').filter_map(|t| t.trim().parse().ok()).collect();
    if values.len() == count {
        Ok(values)
    } else {
        Err(format!("{name} needs {count} numbers separated by commas"))
    }
}

fn point(values: &[f64]) -> P {
    p(values[0], values[1])
}

fn write_sample(dir: &str) -> Result<(), String> {
    for (rev, folder) in [('A', "rev-A"), ('B', "rev-B")] {
        let out = PathBuf::from(dir).join(folder);
        std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
        for (name, text) in samples::board(rev) {
            std::fs::write(out.join(name), text).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

fn run() -> Result<(), String> {
    let mut launch = Launch::default();
    let mut paths: Vec<PathBuf> = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = |name: &str| args.next().ok_or(format!("{name} needs a value"));
        match arg.as_str() {
            "-V" | "--version" => {
                println!("copperview {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            "-h" | "--help" | "help" => {
                print!("{HELP}");
                return Ok(());
            }
            "install" if launch == Launch::default() && paths.is_empty() => {
                platform::install()?;
                println!("Copperview is installed. Find it in your app launcher, or run: copperview");
                println!("To remove it later, run: copperview uninstall");
                return Ok(());
            }
            "uninstall" if launch == Launch::default() && paths.is_empty() => {
                platform::uninstall()?;
                println!("Copperview has been removed. Your Gerber files weren't touched.");
                return Ok(());
            }
            "--screenshot" => launch.screenshot = Some(PathBuf::from(value("--screenshot")?)),
            "--view" => launch.view = Some(value("--view")?),
            "--mix" => launch.mix = Some(value("--mix")?),
            "--unit" => launch.unit = Some(value("--unit")?),
            "--all-layers" => launch.all_layers = true,
            "--flip" => launch.flip = true,
            "--no-details" => launch.no_details = true,
            "--3d" => launch.three_d = true,
            "--play" => launch.play = Some(value("--play")?),
            "--pick" => launch.pick = Some(point(&numbers(&value("--pick")?, 2, "--pick")?)),
            "--zoom" => {
                let v = numbers(&value("--zoom")?, 3, "--zoom")?;
                launch.zoom = Some((point(&v), v[2].max(0.01)));
            }
            "--ruler" => {
                let v = numbers(&value("--ruler")?, 4, "--ruler")?;
                launch.ruler = Some((point(&v), point(&v[2..])));
            }
            "--write-sample" => return write_sample(&value("--write-sample")?),
            other if other.starts_with("--") => return Err(format!("I don't know {other}")),
            path => paths.push(PathBuf::from(path)),
        }
    }
    let mut paths = paths.into_iter();
    launch.a = paths.next();
    launch.b = paths.next();

    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Copperview")
        .with_app_id("copperview")
        .with_inner_size([1440.0, 900.0])
        .with_min_inner_size([560.0, 400.0])
        .with_drag_and_drop(true);
    if let Some((rgba, side)) = icons::picture_pixels(128) {
        viewport = viewport.with_icon(egui::IconData { rgba, width: side, height: side });
    }
    // Wayland stops drawing a window while the screen is asleep; X11 keeps going,
    // so a picture run uses it when it can. Nothing else is running yet.
    if launch.screenshot.is_some() && std::env::var_os("DISPLAY").is_some() {
        unsafe { std::env::remove_var("WAYLAND_DISPLAY") };
    }
    // The 3D view needs to know what's in front of what, and smooth edges.
    let mut options =
        eframe::NativeOptions { viewport, depth_buffer: 24, multisampling: 4, ..Default::default() };
    // A picture run shouldn't wait for a screen that may be asleep.
    options.glow_options.vsync = launch.screenshot.is_none();
    // Ctrl+C, X and V come through as plain keys, for the desktop's clipboard tools.
    options.clipboard_shortcuts = false;
    eframe::run_native("Copperview", options, Box::new(move |cc| Ok(Box::new(app::App::new(cc, launch)))))
        .map_err(|e| format!("The window couldn't open: {e}"))
}

/// A window program on Windows has no console of its own. Started from one, it
/// borrows that console, so --help, --version and errors still show up there.
#[cfg(windows)]
fn use_parent_console() {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn AttachConsole(process: u32) -> i32;
    }
    const ATTACH_PARENT_PROCESS: u32 = u32::MAX;
    unsafe { AttachConsole(ATTACH_PARENT_PROCESS) };
}

fn main() {
    #[cfg(windows)]
    use_parent_console();
    if let Err(error) = run() {
        eprintln!("copperview: {error}");
        std::process::exit(1);
    }
}
