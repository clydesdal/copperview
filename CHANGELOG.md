# Copperview version log

What each version does differently from the one before. Newest first. When
behaviour changes, bump `version` in `Cargo.toml` and add an entry here; the
About screen shows this file.

## 0.2.1

- Copy and paste work on macOS (through `pbcopy` and `pbpaste`) and on Windows (through PowerShell); before, only Linux's clipboard tools were used.
- The "Keep Copperview" card, which installs the program, appears only on Linux. On a Mac, `make-macos-app.sh` builds `Copperview.app` to drag into Applications.
- On a Mac the boards last opened are remembered in `~/Library/Application Support/copperview`; they are forgotten once when you first run this version.
- On a Mac, About Copperview in the app menu opens the same About window as the name at the top left, with the version and this log.
- The same source now builds on Linux, macOS and Windows: system-specific code is in `src/platform/`.

## 0.2.0

### Comparing two boards
- Differences are marked with small yellow location pins whose point sits on the changed object. Differences that are part of the same trace share one pin, and an old and a new piece in the same place share one too, so there are fewer pins.
- On the silkscreen layers and Mechanical 99 (fab notes), differences whose strokes touch, or come within 0.25 mm of each other, share one pin, so a changed word or outline gets one.
- Nearby differences are always gathered into the same place, whatever order they are found in; before, a chain of pieces could split into two places.
- A selected difference blinks at 1 Hz, alternating between A's pieces (red) and B's pieces (green), starting with A. A small "Showing A" or "Showing B" tag says which.
- Click a pin to select it. The pins step aside, everything else dims, and all the differences connected to that pin light up: red for only in A, green for only in B. Next and Previous light up every difference at that place. Esc, or a click on the board away from the lit-up differences, clears the selection and brings the pins back.
- The Compare card lists the objects that differ (kind, size, position and which side they are only on) for the selected pin or place.
- Each object in the Compare card's list can be clicked. The picked difference blinks on and off at 1 Hz while the rest of the set is shown calmly. Click it again, or change the selection, to stop.
- Differences is a layer of its own in the layer list, with an eye to show or hide the markers and dimming.

### Selecting
- When things are stacked, a click now picks a hole first, then a trace, then a pad, then a fill.

### Traces
- Double-click copper to select the whole trace: everything on that layer that touches it, directly or through other copper.
- Copper cleared away around a pour counts as cleared, so a trace no longer leaks into the pour beside it.
- Double-click a highlighted trace again to follow it through plated holes into the other layers.
- The details show the total length and the length on each layer. Layers the trace only lands on with a pad are not listed.

### Differential pairs
- A trace that runs alongside another is shown with that partner in cyan, with both lengths and the length difference.
- A pair needs the same width, and the same spacing kept for over 95% of the run on each layer. Stretches near pads and vias, and layers where a trace is only a short hop, are not judged.

### Label on the board
- The label on a selected piece follows traces and pairs. With the details panel open it is short; with the panel closed it holds everything the panel would show.
- Labels stay inside the view when the piece is long.

### Other
- About screen: click the Copperview name at the top left. It shows the version and this log.
- `copperview --version` prints the version.
- `--no-details` starts with the details panel folded away.

## 0.1.0

- Look at PCB Gerber and drill files from a folder or zip, with Altium file names recognised.
- Measure by clicking pads, tracks, holes and shapes; ruler, grid, units, flip, 3D view.
- Mark up the board with arrows, words and shapes.
- Compare two sets: swipe, differences, upper/lower, side by side, with changes found per layer and stepped through with N and P.
- Install to the Applications folder and app launcher on Linux.
