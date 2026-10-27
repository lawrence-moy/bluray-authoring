# bdauthor (Rust)

Trois crates :
- `bdauthor-core` : format Blu-ray, encodage via ffmpeg, projet (sans dependance externe) — `cargo test`
- `bdauthor-render` : dessin des menus (crates `image`, `ab_glyph`) + `bdauthor-build projet.json sortie`
- `bdauthor-gui` : interface graphique (egui/eframe, rfd) — programme `bdauthor`

Lancer l'interface : `cd bdauthor-gui && cargo run --release`
ffmpeg.exe et ffprobe.exe : a cote du programme (`target\release`), ou variable `BDAUTHOR_FFMPEG_DIR`, ou dans le PATH.
