# Copperview

A gerber viewer for a specific workflow --only really set up for Altium currently. Allows you to compare versions of build files and show specific board changes. Pretty usable now even though it's vibe coded. Adding and changing things fairly rapidly so functions will change.

## Download

[Download the latest release](https://github.com/clydesdal/copperview/releases/latest).
Pick the file for your system:

- Windows: `copperview-setup-<version>.exe` installs it; `copperview.exe` is the program alone
- Mac: `Copperview-<version>-macos.dmg`, for Apple Silicon and Intel
- Linux: `install-copperview-<version>.sh`, run with `sh`

[Installing](#installing) below says what each one does.

## Using it

    copperview                    open the window
    copperview FOLDER_OR_ZIP      open one set
    copperview A B                compare two sets, folders or zips in any mix

Drop a folder or zip on the window, too. `copperview --help` lists every option.

Pinch the trackpad or scroll to zoom. Keys: F fit, M ruler, T mark up, Ctrl+Z undo a mark, K keep a measurement, G grid, X flip, U units,
N and P next and previous change, Delete remove the selected measurement,
Esc clear.

The sample board is vibe coded and really suck --I'll change it soon, and written as real Gerber text with Altium's
file names. `copperview --write-sample DIR` saves it as files.

## Building

One source builds on Linux, macOS and Windows. `rust-toolchain.toml` pins the
compiler (rustup uses it by itself; anything 1.88 or newer works), and
`Cargo.lock` pins every library, so each machine builds the same program.

    cargo build --release          # any system; the program is target/release/copperview
    sh make-macos-app.sh           # on a Mac: dist/Copperview.app and dist/Copperview-<version>-macos.dmg
                                   # (--universal: one app for Apple Silicon and Intel Macs)
    sh make-installer.sh           # on Linux: dist/install-copperview-<version>.sh
    powershell -ExecutionPolicy Bypass -File make-windows-installer.ps1
                                   # on Windows: dist/copperview.exe and dist/copperview-setup-<version>.exe

Everything that differs between systems is in `src/platform/`, one file for
each, offering the same few things: where settings live, copy and paste, and
installing. `src/pinch.rs` holds the Linux trackpad-pinch code, and each
system's libraries are listed under its own heading in `Cargo.toml`. The
tests also compile the other systems' files, so changing one on a Mac or a
Linux machine is checked against the rest.

Every push and pull request is built and tested on Linux, macOS and Windows by
`.github/workflows/ci.yml`. Each run keeps the Linux installer, the Mac disk
image and the Windows installer and `copperview.exe` as downloads on its page
under Actions. Pushing a tag like `v0.2.2` also puts them on a GitHub release.

The Windows installer is made with Inno Setup 6 (`winget install JRSoftware.InnoSetup`)
from `assets/windows/copperview.iss`. `build.rs` puts the icon from
`assets/windows/copperview.ico` into `copperview.exe`; that needs the Windows
SDK's `rc.exe` (MSVC Rust) or a working MinGW `windres` (GNU Rust). Without one
the build still finishes, with a warning, and the installer's shortcuts carry
the icon anyway. `assets/windows/make-ico.py` remakes the `.ico` from `assets/icons`.

## Versions

`CHANGELOG.md` lists what each version does differently. The About screen (click the Copperview name at the top left) shows the version and that log. When behaviour changes, bump `version` in `Cargo.toml` and add an entry.

## Installing

`sh make-installer.sh` builds one small file, `dist/install-copperview-<version>.sh`.
Run it with `sh` on any Linux computer to add Copperview to `~/Applications` and
the app launcher, or press Install inside the app. `copperview uninstall` takes
it out again. Copy and paste use `wl-clipboard` or `xclip`.

On Windows, run `copperview-setup-<version>.exe`. It installs for you alone, with
no administrator needed, and adds Copperview to the Start menu; it can also add
it to PATH and to the right-click menu of folders and zips. Remove it in
Settings > Apps. `copperview.exe` on its own also runs from anywhere.

On a Mac, open `Copperview-<version>-macos.dmg` and drag Copperview into
Applications. The app isn't signed by Apple, so the first time, right-click it
and choose Open (or allow it in System Settings > Privacy & Security).

Figtree is included under the SIL Open Font License (see assets/fonts/OFL.txt).

## License

Copperview is released under the MIT License (see `LICENSE`). Figtree, the
font in `assets/fonts`, keeps its own license, the SIL Open Font License
(`assets/fonts/OFL.txt`).
