# bdauthor (Rust)

Trois crates, reunies dans un espace de travail Cargo (`Cargo.toml` de ce dossier : un seul `target` et un seul `Cargo.lock`) :
- `bdauthor-core` : format Blu-ray, encodage via ffmpeg, projet (sans dependance externe) — `cargo test`
- `bdauthor-render` : dessin des menus (crates `image`, `ab_glyph`) + `bdauthor-build projet.json sortie`
- `bdauthor-gui` : interface graphique (egui/eframe, rfd) — programme `bdauthor`

Lancer l'interface : `cargo run --release -p bdauthor-gui` (depuis ce dossier) ; les programmes compiles sont dans `target\release`.
ffmpeg.exe et ffprobe.exe : a cote du programme (`target\release`), ou variable `BDAUTHOR_FFMPEG_DIR`, ou dans le PATH.

Scripts (double-clic depuis l'Explorateur) :
- `recompiler.bat` : compile les trois crates en release en une seule commande (`recompiler.bat tests` lance aussi les tests du coeur).
- `lancer.bat` : lance l'interface (la compile si besoin) ; le ffmpeg du dossier `code\ffmpeg-*` est utilise automatiquement.
