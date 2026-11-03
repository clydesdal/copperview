# Copperview

Look at PCB Gerber files, measure anything you click, and compare two sets.

    copperview                    open the window
    copperview FOLDER_OR_ZIP      open one set
    copperview A B                compare two sets, folders or zips in any mix

Drop a folder or zip on the window, too. `copperview --help` lists every option.

Pinch the trackpad or scroll to zoom. Keys: F fit, M ruler, T mark up, Ctrl+Z undo a mark, K keep a measurement, G grid, X flip, U units,
N and P next and previous change, Delete remove the selected measurement,
Esc clear.

The sample board is made up, and written as real Gerber text with Altium's
file names. `copperview --write-sample DIR` saves it as files.

## Building

One source builds on Linux, macOS and Windows. `rust-toolchain.toml` pins the
compiler (rustup uses it by itself; anything 1.88 or newer works), and
`Cargo.lock` pins every library, so each machine builds the same program.

    cargo build --release          # any system; the program is target/release/copperview
    sh make-macos-app.sh           # on a Mac: dist/Copperview.app
    sh make-installer.sh           # on Linux: dist/install-copperview-<version>.sh

Everything that differs between systems is in `src/platform/`, one file for
each, offering the same few things: where settings live, copy and paste, and
installing. `src/pinch.rs` holds the Linux trackpad-pinch code, and each
system's libraries are listed under its own heading in `Cargo.toml`. The
tests also compile the other systems' files, so changing one on a Mac or a
Linux machine is checked against the rest.

Every push and pull request is built and tested on Linux, macOS and Windows by
`.github/workflows/ci.yml`. Each run keeps the Linux installer, the Mac app
(zipped) and `copperview.exe` as downloads on its page under Actions.

On Windows the program is just `copperview.exe`; there is no installer yet, and
the Windows code is checked by the tests but hasn't been run on a Windows machine.

## Versions

`CHANGELOG.md` lists what each version does differently. The About screen (click the Copperview name at the top left) shows the version and that log. When behaviour changes, bump `version` in `Cargo.toml` and add an entry.

## Installing

`sh make-installer.sh` builds one small file, `dist/install-copperview-<version>.sh`.
Run it with `sh` on any Linux computer to add Copperview to `~/Applications` and
the app launcher, or press Install inside the app. `copperview uninstall` takes
it out again. Copy and paste use `wl-clipboard` or `xclip`.

Figtree is included under the SIL Open Font License (see assets/fonts/OFL.txt).

## License

Copperview is released under the MIT License (see `LICENSE`). Figtree, the
font in `assets/fonts`, keeps its own license, the SIL Open Font License
(`assets/fonts/OFL.txt`).
