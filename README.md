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

## Versions

`CHANGELOG.md` lists what each version does differently. The About screen (click the Copperview name at the top left) shows the version and that log. When behaviour changes, bump `version` in `Cargo.toml` and add an entry.

## Installing

`sh make-installer.sh` builds one small file, `dist/install-copperview-<version>.sh`.
Run it with `sh` on any Linux computer to add Copperview to `~/Applications` and
the app launcher, or press Install inside the app. `copperview uninstall` takes
it out again. Copy and paste use `wl-clipboard` or `xclip`.

Figtree is included under the SIL Open Font License (see assets/fonts/OFL.txt).
