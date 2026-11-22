# 1. Prise en main

## Installation

bdauthor est un programme autonome (`bdauthor.exe` sous Windows). Il a besoin de **ffmpeg** pour lire et encoder les vidéos :

- `ffmpeg` avec l'encodeur vidéo **libx264** et l'encodeur audio **ac3** ;
- `ffprobe`, qui lit la durée et les pistes des fichiers.

Les versions « essentials » ou « full » de ffmpeg pour Windows conviennent. bdauthor cherche ffmpeg dans cet ordre :

1. le dossier indiqué dans **Fichier › Configuration…** ;
2. le dossier du programme ;
3. le dossier indiqué par la variable d'environnement `BDAUTHOR_FFMPEG_DIR` ;
4. le `PATH` du système.

Le plus simple est de copier `ffmpeg.exe` et `ffprobe.exe` à côté de `bdauthor.exe`.

> 📷 **Capture à faire — `img/config-ffmpeg.png`** : fenêtre *Configuration*, partie « Chemin de ffmpeg », avec la ligne « Utilisé : … » en vert.

### Compiler depuis les sources

Avec [Rust](https://rustup.rs/) installé (sous Windows, avec les outils MSVC) :

```bash
cd rust
cargo run --release -p bdauthor-gui
```

Sous Windows, `rust\recompiler.bat` compile tout en mode release et `rust\lancer.bat` démarre l'application.

## La fenêtre principale

> 📷 **Capture à faire — `img/fenetre-principale.png`** : fenêtre complète sur l'onglet *Vidéos*, avec un projet de trois vidéos, la barre de menus et les trois onglets visibles.

En haut, la **barre de menus** :

| Menu | Contenu |
|---|---|
| **Fichier** | Nouveau projet, Ouvrir projet…, Projets récents, Enregistrer (Ctrl+S), Enregistrer sous…, Configuration…, Quitter |
| **Édition** | ↶ Annuler (Ctrl+Z), ↷ Rétablir (Ctrl+Y), Projet…, Générer le disque, Vérifier les menus… |
| **À propos** | version, licences, emplacement des réglages |

À droite des menus, trois **onglets** correspondent aux trois grandes étapes du travail :

| Onglet | Rôle | Chapitre |
|---|---|---|
| **Vidéos** | le contenu du disque : vidéos, chapitres, pistes audio, sous-titres | [2](02-videos.md) |
| **Mise en page** | l'éditeur des menus du disque | [3](03-editeur-menus.md) |
| **Jaquette** | la jaquette du boîtier et l'étiquette du disque | [4](04-jaquette.md) |

La barre affiche aussi le nom du projet (ou « projet non enregistré ») et l'heure de la dernière sauvegarde automatique.

À droite, la **jauge d'occupation du disque** estime en direct la place que prendra le disque (par exemple « BD-25 · 18,2 / 25,0 Go »). La barre est verte, orange au-delà de la marge de sécurité, rouge si le contenu ne tient pas ; le trait vertical marque la marge. Au survol, le détail : vidéos, audio, sous-titres, menus et multiplexage. La taille des vidéos encodées est une estimation (d'après la qualité CRF et la définition) ; avec « Viser un débit qui remplit le disque », les vidéos prennent la place restante. Le disque visé et la marge se règlent dans **Édition › Projet… › Génération ISO**.

> 📷 **Capture à faire — `img/jauge-disque.png`** : jauge d'occupation dans la barre du haut, avec son détail affiché au survol.

> **Annuler / Rétablir** concernent l'onglet *Mise en page*. L'éditeur de jaquette a son propre historique (mêmes raccourcis).

## Projets

Un projet est un fichier **JSON** lisible. Il contient les réglages et les chemins des fichiers utilisés, mais pas les vidéos elles-mêmes. Les chemins peuvent être relatifs au dossier du projet : gardez le projet et ses médias dans un même dossier pour pouvoir le déplacer facilement.

- **Fichier › Nouveau projet** repart d'un projet vide.
- **Fichier › Ouvrir projet…** ouvre un fichier `.json` ; **Projets récents** liste les derniers ouverts (**Effacer la liste** les oublie).
- **Enregistrer** (Ctrl+S) et **Enregistrer sous…**.

### Sauvegarde automatique et récupération

Toutes les quelques minutes (réglable dans **Fichier › Configuration…**), si le projet a changé, bdauthor écrit une copie à côté du projet, sous le nom `nom.autosave.json`. Le projet lui-même n'est modifié que par **Enregistrer**.

Si le programme s'arrête avant l'enregistrement, la fenêtre **Récupération du projet** apparaît à la réouverture :

- **Restaurer** charge la sauvegarde (enregistrez ensuite le projet pour la conserver) ;
- **Ignorer** garde la sauvegarde pour la prochaine fois ;
- **Supprimer la sauvegarde** l'efface.

> 📷 **Capture à faire — `img/recuperation.png`** : fenêtre *Récupération du projet* avec ses trois boutons.

### Quitter

Si le projet a des modifications non enregistrées, ou si une génération est en cours, bdauthor demande : **Enregistrer et quitter**, **Quitter sans enregistrer** ou **Annuler**.

## Le déroulement type

1. **Vidéos** : ajoutez les vidéos et réglez-les ([chapitre 2](02-videos.md)).
2. **Langues** : dans **Édition › Projet…**, onglet *Langues des sous-titres*, créez la liste des langues du disque (elle sert aussi aux pistes audio).
3. **Mise en page** : créez les pages de départ ou dessinez vos pages ([chapitre 3](03-editeur-menus.md)).
4. **Vérifier les menus** et **Simuler la télécommande**.
5. **Réglages du projet** : qualité, disque visé, image ISO ([chapitre 5](05-projet-et-generation.md)).
6. **Jaquette** ([chapitre 4](04-jaquette.md)).
7. **Générer le disque**, vérifier, tester, graver.
