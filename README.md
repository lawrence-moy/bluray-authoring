# bdauthor

**Create real Blu-ray discs with menus (HDMV) from your videos, using a graphical editor and no other authoring software.**

bdauthor takes your video files, audio tracks and subtitles, lets you design the menus in a visual editor, then produces a complete `BDMV` folder (or an ISO image) that Blu-ray players and playback software can read. It also designs the **cover and the disc label** and exports them for printing. It is written in **Rust**, drives **ffmpeg** for encoding, and its core has **no external dependencies**.

> ⚠️ Under active development. Generated discs are checked by a built-in verifier and play in VLC; not all standalone players have been tested yet. See [Project status](#project-status).

![Videos: audio tracks, subtitles and chapters](screenshots/capture2.jpg)

![Menu editor: pages, layers and navigation buttons](screenshots/capture3.jpg)

![Cover editor: layers, justified text, information table and barcode](screenshots/capture4.jpg)

---

## Contents

- [Features](#features): [Menus](#menus) · [Video, audio and subtitles](#video-audio-and-subtitles) · [Cover and disc label](#cover-and-disc-label) · [Disc output](#disc-output) · [Convenience](#convenience)
- [Settings](#settings)
- [Widgets](#widgets)
- [Architecture](#architecture) · [Installation](#installation) · [Quick start](#quick-start)
- [Project format](#project-format) · [Project status](#project-status) · [Fonts](#fonts)

---

## Features

The application is organized in tabs: **Videos** (content of the disc), **Layout** (menu editor) and **Cover** (cover and disc label), plus the **File**, **Edit** and **About** menus.

### Menus
- **Visual menu editor**: place, move and resize elements with the mouse, with snapping, guides and stacking order (front / back). Several elements can be selected at once, aligned, centered and distributed.
- **Pages and layers**: a tree of pages and layers, with rename, hide, reorder and "home page" commands; copy, cut and paste of widgets (also between pages).
- **Widgets**: see [Widgets](#widgets).
- **Real Blu-ray buttons**: button-to-button navigation computed automatically (or set by hand: up / down / left / right), automatic menu pagination, chapter menus with thumbnails.
- **Button actions**: play a video (from the start or from a chapter or a free time), play all videos in a row, go to a page, the home page or the previous / next page, choose the audio language, set or hide the subtitles; what happens after playback (return to the page, go to another page, stay).
- **Button appearance**: a project-wide button style (see [Settings](#settings)) that each button can override.
- **Menu backgrounds**: still image, or **animated background video** (looped or stopped on its last frame), with **menu music** or the video's own sound; a default for the project and one per page.
- **Opening sequence**: a video played before the menu (without one, the disc opens directly on the menu).
- **Overlap detection**: a blink warns about overlapping elements (red) or text hidden behind a button (orange).
- **Faithful preview**: what the editor shows is what gets authored, plus a **navigation simulator** to test the buttons before building. Pan the preview by holding the right mouse button.
- **Per-page automatic launch**: a page can start one of its buttons by itself after a delay if the viewer does nothing (the delay is a whole number of background loops).
- **Menu resolution**: 1920×1080 by default, or the size of the first video, or a custom 16:9 size.

### Video, audio and subtitles
- **Video files**: mp4, mkv, mov, avi, m2ts, mts, ts, webm, wmv, flv, m4v, mpg, mpeg; drag to reorder the videos; reload a video to re-read its duration and tracks.
- **Encoding through ffmpeg** (H.264 / AC3) with quality (CRF) and preset settings, an optional **fill the disc** mode (same average bitrate for all videos, two passes, to use the disc capacity), optional **copy of Blu-ray-compliant H.264 videos** (video stream kept as is, audio converted to AC-3), an encoding cache, cancellation, and **detailed progress** (step, elapsed time, ETA, ffmpeg detail).
- **Up to 8 audio tracks per video**, each with its own language (several tracks of a video can come from other files, with an offset); reorder them by drag and drop; a listening window with a waveform to adjust offsets.
- **Audio language buttons**: the disc remembers the chosen *language*, and each video selects its own matching track at start. A default audio language is set for the project.
- **Up to 8 subtitle tracks per video**: SRT and ASS/SSA files, or subtitles extracted from the video file; automatic detection of the **text encoding** (UTF-8, Windows code pages, Cyrillic, Greek, Hebrew, Arabic, Thai, Japanese, Chinese, Korean…).
- **Subtitle style**: font, size, color, outline, shadow, opacity, background box with rounded corners, position, margins, alignment, maximum width and line wrapping. A **preview window** with a playback timeline shows the result on the video frame, with the safe area.
- **Subtitle quality check**: warns about reading speed, line length, number of lines, too short or too long display times and too small gaps, with adjustable limits.
- **Project-wide list of subtitle languages**, reorderable by drag and drop.
- **Chapters**, manual or automatic (every N seconds); a **chapter editor** with the video image by image, playback and a timeline (timecode `hh:mm:ss:ff`), where chapters and menu thumbnails are placed; a chapter can have its own page thumbnail.
- **Thumbnails** of the timelines are computed in parallel, kept in a disk cache and prefetched in the background, so the editors open at once.

### Cover and disc label
- **Cover editor** (back, spine, front, with bleed) for standard and thick Blu-ray cases, standard and slim DVD cases, and slim Blu-ray cases, or a custom format.
- **Disc label editor** (disc with its center hole).
- **Widgets**: *Text*, *Image*, *Video image*, *Information table*, *Barcode*, *Rectangle*, *Ellipse* and *Line* (see [Widgets](#widgets)).
- **Layers**: tree of layers to hide, lock, rename and reorder items; drag items between layers.
- **Guides**: safe margin, snapping, rulers in **mm or inches**, optional grid, adjustable background and guide colors, panning with the right mouse button, wheel zoom.
- **Automatic fields** in texts: disc title, list of videos (number, title, duration), number of videos, total duration, number of chapters, current year.
- Alignment of one or several items (to each other or to the back / spine / front / disc), distribution, duplicate, copy / paste, rotation (0°, 90°, 270°), undo / redo.
- Export to **PDF** (actual size) or **PNG** (optionally trimmed to the finished format, without bleed), or **Print on A4**: an A4 PDF with the cover at actual size, centered, with crop marks, to print at 100 % and cut out.

### Disc output
- Complete **`BDMV`** folder or **ISO image** (UDF 2.50), with a volume name, owner, organization and contact fields, a chosen or automatic date, and layout options (alignment on 64 KB, files ordered by playback, maximum compatibility, mirrored metadata).
- **Target disc capacity** with a safety margin and a warning when the content does not fit.
- **Built-in verifier** (`bdcheck`) that checks the consistency of the generated disc (index, playlists, clips, menus) and of the ISO image.
- "Generate the disc" window with a live log, and the project is checked for problems before the build starts.
- Discs are **region-free and unprotected**: no AACS, BD+ or region code (A/B/C), so they play on any player. Region locking and copy protection require commercial licences and are not supported.

### Convenience
- **Bilingual interface** (French / English); an **interface translation** file can be created to add another language (put it in the `langues` folder).
- Projects saved as **human-readable JSON** with English keys; **recent projects** in the File menu.
- **Automatic backup** of the project every few minutes, offered for restoration after an unexpected stop.
- Undo / redo, copy-paste of elements, remembered settings.

---

## Settings

**General settings** (*File › Settings*, stored on the computer, not in the project):
- interface **language**;
- **ffmpeg** folder (otherwise: next to the program, then the `PATH`);
- **additional fonts folder** (`.ttf` and `.otf`; added to every font list; a "?" lists the supported formats);
- **thumbnail cache folder** (size shown, "Default", "Clear" and "Open" buttons);
- **automatic project backup** interval;
- preview refresh pace while dragging;
- list of recent projects.

**Project settings** (*Edit › Project…*, saved with the project):

| Section | What you set |
|---|---|
| **Disc** | disc title, opening sequence, default audio language, menu size (1920×1080, first video, custom 16:9) |
| **Encoding** | x264 quality (CRF) and preset, *fill the disc* up to a share of the capacity, copy of compliant videos, reuse of already encoded videos (cache) |
| **Menu background and music** | project default image or looped video, duration, music, keep the video's sound |
| **Button style** | unselected / selected / activated look of every button: background, text, border, darkening, glow (animated pulse), enlargement, corner rounding, sounds, optional hand-drawn images or animations |
| **Page style** | default look of the menu pages |
| **Subtitle languages** | ordered list of subtitle languages |
| **ISO generation** | build the BDMV folder, the ISO image or both; volume name and metadata; target disc and filling limit; UDF version; layout options; date |

---

## Widgets

### Menu widgets

| Widget | Use |
|---|---|
| **Text** | free or clickable text; font, size, color, alignment, outline, shadow |
| **Image** | PNG, JPEG or SVG image, optionally clickable |
| **Video thumbnail** | a frame of a video, chosen automatically or at a chosen moment |
| **Video image** | any frame of a video, with optional crop |
| **Navigation buttons** | previous / next page, home, play all, go to… |
| **Rectangle** | fill, border, rounded corners, adjustable opacity |
| **Line** | edited through its two endpoints; solid, dashed, dotted, double; arrows; 45° angle snapping |

### Cover and label widgets

| Widget | Use |
|---|---|
| **Text** | font, size, color, alignment (left / center / right), **justification** (text cut into lines as wide as the frame, words spaced to fill each line; the last line of a paragraph keeps its alignment), vertical position, line spacing, *fit to frame*, bold, italic, outline, shadow; automatic fields |
| **Image** | PNG, JPEG, SVG |
| **Video image** | a frame of one of the project's videos (same picker as in the menu editor) |
| **Information table** | duration, languages, subtitles, format and sound of one or several videos, read from the project; choose and order the videos (drag and drop) and the columns, headings, one row per information, size, colors, separate background for headings and cells, grid, font |
| **Barcode** | **Code 128, EAN-13, EAN-8 and UPC-A** drawn as vector bars; the check digit is added or verified; readable text with its own font and size; color and background; variables such as `{title}` accepted; the shape stays identical at any zoom or resolution |
| **Rectangle**, **Ellipse** | fill, border, rounded corners (rectangle) |
| **Line** | endpoints, dashes, arrows |

---

## Architecture

| Crate | Role | Dependencies |
|---|---|---|
| `bdauthor-core` | Blu-ray formats (index, MovieObject, playlists, CLPI, M2TS, IG), muxing, project model, cover model and barcodes, editor geometry, verifier, ISO, JSON, PNG, PDF | **None** (std only) |
| `bdauthor-render` | Menu and cover drawing (text, shapes, images, SVG) | `image`, `ab_glyph`, `resvg` |
| `bdauthor-gui` | `bdauthor` graphical application | `egui` / `eframe`, `rfd` |

The three crates form one Cargo workspace (`rust/Cargo.toml`).

Command-line tools: `bdauthor-build` (project → disc), `bdcheck` (verification), `bdinspect` (BDMV file inspection), `bdiso` (ISO image), `bdauthor-ffmpeg` (ffmpeg tests).

---

## Installation

### Requirements
- [Rust](https://rustup.rs/) (stable toolchain; on Windows, with the MSVC tools)
- [ffmpeg](https://ffmpeg.org/) **with `libx264` and the `ac3` encoder**, plus `ffprobe` (and `ffplay` for audio track listening)

ffmpeg is looked up in this order: the folder set in the settings, next to the executable, the folder given by the `BDAUTHOR_FFMPEG_DIR` environment variable, then the `PATH`.

### Build and run
```bash
cd rust
cargo run --release -p bdauthor-gui
```
On Windows, `rust\recompiler.bat` builds everything in release mode and `rust\lancer.bat` starts the application.

### Build a disc from the command line
```bash
cd rust
cargo run --release -p bdauthor-render --bin bdauthor-build -- project.json output_folder
```

### Tests
```bash
cd rust
cargo test -p bdauthor-core
```

---

## Quick start

1. **File › New project**, then add your videos.
2. For each video: title, audio tracks (language), subtitles, chapters, thumbnail image.
3. Open the **Layout** tab: lay out the page (text, images, buttons, shapes) and test it with the simulator.
4. Set the encoding quality and the target disc in the project settings.
5. Open the **Cover** tab to design the cover and the disc label; export them to PDF or PNG.
6. **Build the disc**: follow the progress, then read the verification report.
7. Open the output folder in VLC, or burn the ISO image.

---

## Project format

Projects are readable JSON files you can edit by hand (relative paths allowed). Unknown fields are preserved on save.

```json
{
  "title": "My disc",
  "language": "eng",
  "videos": [
    { "file": "movie1.mp4", "title": "Movie 1", "auto_chapters_s": 60 }
  ],
  "encoding": { "crf": 18, "preset": "medium" }
}
```

Keys are in English. Project files written by earlier versions with French keys (`titre`, `fichier`, `encodage`…) are no longer read.

---

## Project status

- **Verified**: navigation files and muxing output were compared byte for byte with a reference implementation and with a disc produced by another tool; the built-in verifier validates generated discs; playback works in VLC.
- **To be validated**: playback on standalone players, audio language switching on real hardware, sources with other frame rates (PAL, etc.), scanning of printed barcodes with a real reader.
- **Known limits**: files are read into memory during muxing; the encoding cache does not keep encoding statistics.

Detailed technical notes (format, pitfalls, tests) are in [`NOTES.md`](NOTES.md).

---

## Fonts

bdauthor does not bundle any font. Menus, covers and labels are drawn with a font file found on the computer: the `BDAUTHOR_FONT` environment variable, then the system fonts (Arial, Segoe UI or Calibri on Windows; DejaVu or Arial elsewhere). Text items can use any installed font, any `.ttf` / `.otf` file of the **additional fonts folder** set in the general settings, or any font file chosen with *Other file…*. In a project, a font is saved by its file name: on another computer, put the same fonts in its additional fonts folder. System fonts are subject to their own licences; for discs and covers you plan to distribute, prefer freely licensed fonts (SIL OFL, e.g. Liberation, DejaVu, Noto).

---

## Contributing

Feedback, bug reports (ideally with the build log) and suggestions are welcome through Issues.

## License

[MIT](LICENSE)
