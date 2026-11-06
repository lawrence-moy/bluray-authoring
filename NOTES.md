# NOTES.md : logiciel d'authoring de menus Blu-ray (HDMV)

> À coller au début d'une nouvelle conversation, avec les fichiers de `bdauthor/` si besoin.
> État au 21/10/2026 (mis à jour après validation des chapitres).

## 1. Objectif et choix retenus

Logiciel léger et multiplateforme qui crée des menus Blu-ray **HDMV** (graphiques IG, pas BD-J) pour des vidéos perso : menu principal, chapitres, éventuellement audio et sous-titres.

- Langage final : Rust. Prototypage actuel : **Python** (portage Rust plus tard).
- Interface prévue : Tauri.
- Approche : piloter ffmpeg / x264 / tsMuxeR pour la vidéo, et écrire nous-mêmes la partie menu, navigation et multiplexage du flux IG.
- Environnement de l'utilisateur : Windows, **sans WSL** (l'explorateur affiche une entrée « Linux », mais l'utilisateur confirme ne pas avoir WSL).
- Forfait gratuit : conversations ciblées, une fonction ou un fichier à la fois.

## 2. Disque de référence (créé avec multiAVCHD 4.1.77)

Dossier `Authoring/AVCHD/BDMV` : `AUXDATA/`, `BACKUP/`, `BDJO/` (vide), `CLIPINF/`, `JAR/` (vide), `META/`, `PLAYLIST/`, `STREAM/`, `index.bdmv`, `MovieObject.bdmv`.

Fichiers analysés : `index.bdmv`, `MovieObject.bdmv`, `PLAYLIST/00000.mpls` (le seul), `CLIPINF/00000.clpi`, `CLIPINF/80002.clpi`, `STREAM/80002.m2ts`.
Non analysé : `STREAM/00000.m2ts` (le film, environ 17,8 Mo).

Tous les fichiers de navigation sont en **version `0100`** (pas `0200` comme je l'avais d'abord dit).

### Chaîne de navigation constatée
- `index.bdmv` (120 octets) : First Playback → objet 255, Top Menu → objet 259, 1 titre → objet 1. Les trois sont HDMV. Le titre a un `playback_type` de 0 (les deux autres : 1), détail non expliqué.
- `MovieObject.bdmv` (23 570 octets, **264 objets**, génération multiAVCHD pilotée par registres) :
  - objet 255 : initialise des registres puis `JUMP_OBJECT 257` ;
  - objet 257 : `MOVE r2066, 1` puis `JUMP_OBJECT 256` (objet 256 pas encore regardé) ;
  - objet 259 : aiguilleur sur r3 / r4 (sauts vers 258, 260, 261, 262, 263 ou `PLAY_PL`), puis boucle d'attente ;
  - **démarrage du disque de référence (suivi complet)** : 255 (met r1 = 254, r2 = 1, r3 = 65535) → 257 → 256 → `JUMP_TITLE 0` (Top Menu) → 259, qui, avec r3 = 65535, ne dispatche rien et exécute `PLAY_PL r1`, soit la **playlist 254**, en boucle (`GOTO 19`). Or `PLAYLIST/` ne contient que `00000.mpls` : **le disque d'origine démarre sur une playlist de menu absente, donc rien ne se lit** (constaté par l'utilisateur dans VLC). Seul le titre 1 (objet 1) lance le film (playlist 0). Déduction par lecture du code, pas encore confirmée sur lecteur.
  - objet 1 (titre) : le `GOTO 3` est inconditionnel et saute `PLAY_PL 253` ; il aboutit à `PLAY_PL_PM 0, r10`, donc la **playlist 0**.
- `00000.mpls` : 1 PlayItem (clip `00000`, 600,000 s → 609,676 s, soit 9,7 s), 1 marque (chapitre à 600 s), **1 SubPath de type 3** (menu IG) avec 1 SubPlayItem (clip `80002`, 0,477 s → 0,511 s).
  Table des flux (décodée à la main, pas encore dans le code) : H.264 PID `0x1011`, AC3 PID `0x1100` (« und »), IG PID `0x1400` (« eng », dans le SubPath).
  Anomalie à confirmer : le champ de longueur de la table STN annonce 46 octets, ce qui ne couvre pas l'entrée IG qui suit. La longueur du PlayItem, elle, est cohérente.
- `00000.clpi` : type d'application 1 (film), 92 576 paquets de 192 octets, PCR PID `0x1001`, PMT PID `0x0100`, H.264 (`0x57` : 720p, 59,94) et AC3, EP_map de 19 points (un toutes les 0,53 s, premier à 600 s, paquet 4).
- `80002.clpi` : type d'application 5 (sous-TS de menu interactif), 896 paquets, un seul flux IG `0x1400`, **pas d'EP_map**, séquence 0,477 s → 0,511 s.
- `80002.m2ts` (172 032 octets) :
  - PIDs : 0 (PAT), `0x100` (PMT), `0x1f` (1 paquet), `0x1001` (**2 paquets PCR**, adaptation seule, début et fin), `0x1400` (872 paquets IG), `0x1fff` (19 paquets de bourrage) ;
  - 96 PES sur le PID IG, un segment par PES : **1 ICS** (19 652 o), **1 PDS** (255 couleurs), **93 ODS**, **1 END** ;
  - stream_id PES `0xBD`, PTS et DTS en 90 kHz ; PTS de l'ICS = 42 956 (0,477 s, identique au SubPlayItem) ; DTS du premier PES = 19 628 (0,218 s) ;
  - ICS : 1920×1080, composition_state `0x80` (début d'epoch), stream_model = 1, ui_model = 1 (**menu pop-up**, confirmé visuellement : barre « Title Menu / Chapters Menu / Title List / Previous Title / Next Title / X »), 4 pages (0 : 2 boutons invisibles « moteur » à commandes automatiques ; 1 : 16 ; 2 : 27 ; 3 : 12), jusqu'à 94 commandes par bouton (240 pour la page 0).

### Horloges (piège fréquent)
- Playlists et table des séquences du `.clpi` : **45 kHz**.
- PES (PTS/DTS) et EP_map : **90 kHz**.
- PCR et ATS : **27 MHz**.

## 3. Code (dossier `bdauthor/`)

```
bdauthor/
  bdauthor/
    reader.py   lecture big-endian + extraction de bits
    index.py    index.bdmv
    mobj.py     MovieObject.bdmv + désassembleur de commandes
    mpls.py     playlists
    clpi.py     infos de clip + EP_map
    m2ts.py     paquets de 192 octets, PES, découpe en segments
    ig.py       ICS / PDS / ODS, RLE (décodage et encodage), conversion YCrCb -> RGBA
    mux.py      multiplexeur M2TS d'un clip de menu IG (PSI, PCR, ATS, PES, bourrage)
    menu.py     menu : boutons (texte ou images PNG) -> ICS + PDS + ODS -> .m2ts + .clpi ; voisins calculés (auto_neighbors)
    png_palette.py  quantification RGBA -> palette BD commune (Y/Cr/Cb + alpha par paliers), BT.709 plage limitée
    vmux.py     multiplexeur M2TS de film (H.264 + AC3) : PSI, PCR, ATS, PES, entrelacement, bourrage
    encode.py   vidéo quelconque -> ffmpeg (x264 bluray-compat + AC3) -> notre multiplexeur (CLI : python -m bdauthor.disc video.mp4 sortie/)
    project.py  disque multi-titres depuis un projet JSON (vignettes, fond de menu dédié, navigation à N titres)
    check.py    vérificateur de dossier BDMV (références croisées, commandes des boutons IG, repères) : python -m bdauthor.check chemin/BDMV
    hdmv_menu_builder.py  ancien appel build_custom_menu_clip(normal, selected, activated, m2ts, clpi), réécrit sur les modules validés
    nav.py      index.bdmv, MovieObject.bdmv, playlist .mpls (menu en SubPath) à partir de paramètres
    tsclip.py   analyse d'un .m2ts de film (PSI, SPS H.264, AC3, points d'accès) et génération de son .clpi
    disc.py     assemble un dossier BDMV complet depuis un .m2ts (CLI : python -m bdauthor.disc film.m2ts sortie/)
  tests/        roundtrip.py, roundtrip_mobj.py, roundtrip_mpls.py,
                roundtrip_clpi.py, inspect_ig.py, remux_ig.py, build_menu.py, build_disc.py, build_disc_accueil.py,
                build_always_on.py, build_bisect.py, gen_clpi.py, build_disc_complet.py, build_menu_png.py, film_mux_check.py, build_project.py, build_chapters.py
  refs/         rendus PNG du menu (80002_page1/2/3.png)
  examples/     boutons PNG d'exemple (dégradés, bords adoucis, 3 états)
  out/          menu généré (80002.m2ts, 80002.clpi, apercu_menu.png) ; bdmv_overlay/ et bdmv_overlay.zip = fichiers à superposer au disque de référence
  NOTES.md
```

Lancer un test : `python -m tests.roundtrip_clpi chemin/00000.clpi` (depuis `bdauthor/`).

Règle de méthode : chaque parseur a son écriveur, et on vérifie que `write(parse(fichier))` redonne le fichier **octet pour octet**.

### État de vérification
Aller-retour identique obtenu sur : `index.bdmv`, `MovieObject.bdmv`, `00000.mpls`, `00000.clpi`, `80002.clpi`, et sur chaque segment ICS / PDS / ODS de `80002.m2ts`. Les 93 images ODS se décodent avec les bonnes dimensions et s'affichent correctement.

**Re-multiplexage (fait)** : `python -m tests.remux_ig 80002.m2ts` recalcule les 96 couples PTS/DTS à partir des segments, puis régénère le `.m2ts` : résultat **identique à l'octet près** (172 032 octets) quand les PCR sont placés aux positions du disque de référence. Avec la politique par défaut (PCR toutes les 90 ms), le fichier diffère (le 2e PCR de la référence, au paquet 863, n'est pas reproduit, d'où un décalage de paquets).

**Menu minimal généré (fait)** : `python -m tests.build_menu 80002.clpi` produit dans `out/` un clip de menu à 2 boutons (« Lire le film » → `JUMP_TITLE 1`, « Menu principal » → `JUMP_TITLE 0`) : 128 paquets, ICS de 138 octets, 6 ODS (3 états par bouton), `.clpi` associé. Relecture par nos parseurs : ICS et images identiques. Le `.clpi` généré avec les paramètres du disque de référence est **identique** au `80002.clpi` d'origine. Mêmes réglages que la référence : ICS 1920×1080, `stream_model=1`, `ui_model=1` (pop-up), voisins de bouton = eux-mêmes ou voisin direct, son `0xFF`. Mot d'instruction `JUMP_TITLE` repris de `MovieObject.bdmv` de référence (`0x21810000`). Masque UO de la page : zéros (la référence utilise `0000053f40000000`, effet non étudié).

**Navigation générée (fait)** : `python -m tests.build_disc 00000.mpls` produit `out/bdmv_overlay/BDMV/` (+ copies dans `BACKUP/`) et `out/bdmv_overlay.zip` : `index.bdmv` (First Playback et Top Menu → objet 0, titre 1 → objet 1), `MovieObject.bdmv` (82 octets, 2 objets qui font chacun `PLAY_PL 0`), `00000.mpls`, `80002.clpi`, `80002.m2ts`. La playlist générée de zéro (paramètres du disque de référence, table des flux reconstruite) est **identique** à `00000.mpls` d'origine, anomalie de longueur STN comprise (reproduite volontairement). Les fichiers du film (`00000.m2ts`, `00000.clpi`) restent ceux du disque de référence : le paquet est conçu pour être copié sur une **copie** de ce disque. Navigation volontairement triviale : tout lance le film, le menu n'est qu'un pop-up pendant la lecture (boutons : `JUMP_TITLE 1` et `JUMP_TITLE 0`). Pas de vrai menu d'accueil à ce stade.

**Clip de film (fait)** : `python -m tests.gen_clpi 00000.m2ts 00000.clpi` analyse le flux (PAT/PMT, SPS H.264 → 1280×720 progressif, cadence 59,94 via les PTS, AC3 stéréo 48 kHz via l'en-tête) et génère `00000.clpi` : **identique octet pour octet** à celui de multiAVCHD (avec son remplissage ASCII '0' des attributs ; avec des zéros, seule cette zone diffère). Règles (dans `tsclip.py`) : points d'accès EP_map = PES vidéo contenant un SPS (19 sur 581 PES, ni les seuls IDR, ni les seules images I) ; PTS d'entrée = PTS du PES ; entrée « coarse » à chaque changement de PTS[32:19] ou SPN[31:17] ; début de séquence = plus petit PTS / 2 ; fin = plus grand PTS vidéo / 2 (sans durée d'image : coïncide aussi avec « dernier PES + 1502 », cas non distinguable) ; débit = floor(taille du fichier / durée de séquence) ; I_end_position_offset = ceil(log2(taille de l'unité d'accès / 64 Kio)) (ajusté sur 19 images, valeurs 2 et 3 seulement observées ; une règle linéaire par paliers de 128 Kio colle aussi : à vérifier). La disposition binaire de l'EP_map et des attributs a été recoupée avec des sources indépendantes (libbluray `clpi_parse.c`, libfreemkv).

**Disque complet autonome (fait, à tester)** : `python -m tests.build_disc_complet 00000.m2ts 00000.clpi` (ou `python -m bdauthor.disc film.m2ts sortie/`) écrit un dossier `BDMV/` entier sans rien reprendre de multiAVCHD : index, MovieObject (menu d'accueil), `00000.mpls` (film), `00001.mpls` (menu, fond = le film), `00000.clpi`, `80002.clpi`, `00000.m2ts` (copié), `80002.m2ts` (menu), copies `BACKUP/`, dossiers vides `AUXDATA/ BDJO/ JAR/ META/` et `CERTIFICATE/`. Playlists et navigation identiques à celles déjà validées dans VLC. Livrable : `out/disque_complet.zip` (17,6 Mo). **Résultat dans VLC : fonctionne parfaitement** (confirmé par l'utilisateur). Premier disque entièrement produit par notre code, hors le flux du film lui-même.

**Vidéo MP4 → disque (fait, à tester dans VLC)** : `python -m bdauthor.disc video.mp4 sortie --lang fra`. Source testée : `GSC_0037.MP4` (H.264 Main, 1280×720 progressif 59,94 i/s, plage pleine `yuvj420p`, 8,56 s, AAC stéréo 48 kHz). Chaîne : ffmpeg (`fps` + `scale` vers BT.709 plage limitée ; libx264 profil High niveau 4.1, `bluray-compat=1`, `keyint=30`, GOP fermé, `aud=1`, `nal-hrd=vbr`, `vbv-maxrate=30000`, `vbv-bufsize=30000`, crf 18 ; AC3 192 kb/s 48 kHz stéréo ; MPEG-TS avec `-pes_payload_size 0` pour une trame AC3 par PES) → relecture des PES par nos outils → remise à `PTS_début = 54 000 000` (600 s) → `vmux.mux_film`. Résultat : 13,27 Mo, 513 images, 268 trames audio, 22 points d'accès, séquence 600,000 → 608,547 s. Contrôles indépendants : ffprobe (High, niveau 4.1, 1280×720, yuv420p tv, BT.709, AC3 192 kb/s), décodage complet sans erreur, 513 images décodées, PSNR 42,8 dB contre la source.
Règles du multiplexeur (`vmux.py`, issues du film de référence) : PAT/PMT/SIT en tête puis toutes les 0,25 s aux frontières de PES (compteurs de continuité par PID) ; PCR dans des paquets dédiés (PID 0x1001) au plus tous les 90 ms (réf. 100,1 ms), valeur = ATS ; débit d'arrivée = peak_rate × 50 = 4 437 500 o/s (35,5 Mb/s, SIT peak_rate 88750) ; les PES sont émis dans l'ordre des heures de décodage (DTS vidéo, PTS audio), chacun arrive ~0,483 s avant (augmenté automatiquement si une image arrivait trop tard) ; PES vidéo : stream_id 0xE0, longueur 0 si > 65 535 o ; PTS seul quand DTS = PTS ; audio en stream_id **0xBD** (la référence utilise 0xFD avec en-tête étendu : écart volontaire, usage courant, non vérifié sur matériel) ; PMT avec descripteurs ES (AVC 0x28, 'AC-3' + descripteur 0x81 + langue) reconstruits **identiques** à ceux de la référence ; bourrage final à 32 paquets.
Validation du multiplexeur par rejeu du film de référence (`python -m tests.film_mux_check 00000.m2ts 00000.clpi`) : 581 PES vidéo et 303 trames audio **identiques**, 0 saut de compteur, PCR == ATS, avance vidéo 0,438 / 0,483 / 0,483 s (réf. 0,419 / 0,483 / 0,534), tampon vidéo estimé 0,99 Mo (CPB 3,75 Mo), `.clpi` : mêmes PTS d'EP_map, mêmes `I_end_position_offset`, même séquence, mêmes flux, même débit.
Non vérifié : conformité T-STD complète (seules des estimations de tampon) ; 1080p / autres cadences (code de `pick_target` écrit, jamais exécuté) ; lecture entière du fichier en mémoire (OK pour quelques minutes, pas pour un long film : prévoir un flux) ; nécessite **ffmpeg + libx264 + encodeur AC3 dans le PATH** (présents ici, à vérifier sous Windows) ; lecteur matériel.

**Résultat utilisateur** : `disque_perso.zip` (une vidéo MP4 encodée par notre chaîne) **fonctionne dans VLC** (confirmé).

**Disque multi-titres (fait, validé dans VLC)** : `python -m bdauthor.project projet.json sortie/` (JSON : `titre`, `langue`, `videos` [{`fichier`, `titre`, `vignette_s`}], `encodage` {`crf`, `preset`}, `menu` {`fond`, `fond_secondes`, `taille`}). 1 à 12 vidéos (au-delà : refusé, pagination non gérée). Vidéos = clips et playlists 0..N-1 = titres 1..N ; fond de menu = clip N (image fixe encodée, **sans audio**, 4–5 s) ; playlist de menu N (fond + sous-chemin IG, boucle par objet 0) ; clip IG 80002 ; plan graphique = taille de la vidéo (1280×720 ici ; option `"taille": "1080"` pour 1920×1080 comme le menu validé). Grille automatique (3×2 jusqu'à 6, 4×3 jusqu'à 12), vignette extraite par ffmpeg, étiquette, état sélectionné = cadre blanc ; état activé = même objet que sélectionné. Palette commune de 254 entrées avec tramage de Floyd-Steinberg (chemin « opaque » de `png_palette.py`). `ics_pts` calculé d'après la durée de décodage des objets (minimum 42956). Navigation : objet 0 = menu en boucle ; objet i = `PLAY_PL i-1` puis `JUMP_TITLE 0`.
**Résultat utilisateur dans VLC (`disque_6titres.zip`)** : menu à six vignettes avec titres, sélection et lancement de la vidéo qui fonctionnent ; fond = dégradé bleu foncé par défaut (voir `menu.fond` pour une image personnelle). Retour au menu en fin de vidéo : OK. Fond = image fixe (aucune répétition visible, normal). **Défaut constaté** : au bout de quelques secondes la sélection revenait à la première vignette (le fond de 4 s est relancé en boucle par l'objet 0, ce qui réinitialise le menu). **Correction** : `still_mode=2` (arrêt infini sur la dernière image) dans la PlayItem de la playlist de menu, option `menu.boucle` (false par défaut) ; ne change qu'un octet de la playlist ; livrée dans `correctif_menu_fixe.zip` pour le disque à 6 titres ; **validé dans VLC par l'utilisateur** : le menu reste affiché et la sélection tient (lecteurs matériels non testés).
Vérifié (`python -m tests.build_project source.mp4 N`) pour N = 1, 6, 12 : vérificateur de références croisées sans erreur ; ffmpeg décode chaque clip sans erreur (144 images pour 2,4 s à 59,94 i/s ; fond 240 images) ; voisins de boutons corrects ; dispositions de 1 à 12 sans débordement ni chevauchement (720p et 1080) ; 13 vidéos refusées. Le clip IG n'est pas décodable par ffmpeg (message « no stream », normal, comme pour la référence).
**Non vérifié** : lecture dans VLC / sur matériel ; taille du plan IG 1280×720 sur le matériel (le menu validé était en 1920×1080) ; clip vidéo **sans audio** (fond) accepté par les lecteurs matériels ; débit du flux IG et taille de l'objet buffer pour un menu de photos (0,7 Mo pour 6 vignettes, 0,8 Mo pour 12 ; limite arbitraire de 6 Mo) ; changement de format entre vidéos de tailles ou cadences différentes (avertissement seulement) ; réencodage de chaque vidéo en entier (pas de mode « copie » si la source est déjà conforme).

**Chapitres (fait, validé dans VLC pour le démarrage au bon repère)** : JSON par vidéo `chapitres` (secondes, ou objets `{t, titre}`) ou `chapitres_auto_s`. 2 à 12 chapitres : (1) repères (`Mark`, type 1) dans la playlist de la vidéo, calés sur l'image d'accès (EP_map) qui précède l'instant demandé, le premier au début du clip ; (2) menu de chapitres par vidéo : fond dédié (image fixe avec « titre - chapitres »), clip IG `80003+k`, playlist `N+1+k` (arrêt sur image infini) ; boutons de chapitre (vignette prise à l'instant du repère, étiquette « Chapitre j (m:ss) »), « Tout lire », « Retour » ; (3) le bouton de la vidéo dans le menu principal ouvre le menu de chapitres (vidéo sans chapitres : lancement direct). Navigation (copie du motif de l'objet 1 de la référence) : bouton de chapitre = `MOVE r0, j` puis `JUMP_TITLE t_lecture` ; titre `t_lecture` = objet `MOVE r10, r0 ; MOVE r0, 0 ; PLAY_PL_PM playlist, r10 ; JUMP_TITLE 0` ; titre de menu = objet `PLAY_PL menu ; NOP×3 ; GOTO 0`. Titres : 1..N vidéos, puis (menu, lecture) pour chaque vidéo à chapitres. Mots d'instruction relevés dans la référence : `MOVE r,imm` 0x50400001, `MOVE r,r` 0x50000001, `PLAY_PL_PM` 0x42820000 (playlist immédiate, repère en registre).
Vérifié sans lecteur (`python -m tests.build_chapters source.mp4`) : vérificateur sans erreur (commandes de tous les boutons, voisins, repères croissants, dans le PlayItem et alignés sur l'EP_map à 512 ticks près), ffmpeg décode les 6 clips (vidéos + fonds), grilles de 1 à 12 éléments avec et sans zone réservée sans débordement ni chevauchement (4 grilles de repli, vignettes de 260 à 480 px).
**Résultat utilisateur dans VLC (`disque_chapitres.zip`)** : les sous-menus de 3 à 4 chapitres s'affichent ; les chapitres démarrent au bon endroit (donc `MOVE r0, j` + `JUMP_TITLE` + `PLAY_PL_PM` à repère en registre fonctionnent : le GPR est conservé) ; la vidéo sans chapitres se comporte comme prévu ; les chapitres automatiques aussi. Remarque : « à la fin du chapitre, on revient au menu général » : retour au menu principal, alors qu'on attendait probablement le menu de chapitres de la vidéo (interprétation confirmée par la suite : retour souhaité vers le menu de chapitres). Correction : `menu.retour` = `chapitres` (défaut : après une vidéo à chapitres, lue en entier ou depuis un chapitre, retour à son menu de chapitres) ou `principal` (ancien comportement) ; ne change que 4 octets de `MovieObject.bdmv` (opérandes de `JUMP_TITLE` des objets 1, 3, 5, 7 pour ce disque) : `correctif_retour_chapitres.zip`. **Validé dans VLC par l'utilisateur** : retour au menu de chapitres, lecture qui traverse les chapitres, comportement conforme à l'attendu.
**Constaté dans VLC** : la liste des chapitres (Lecture > Chapitre) est bien détectée ; **les boutons/touches « suivant » et « précédent » ramènent au menu au lieu de passer d'un chapitre à l'autre** (cause non établie : hypothèse principale = ces boutons de VLC agissent sur la liste de lecture (un seul élément, le disque), qui redémarre alors le disque au First Playback ; autre piste = chapitres en mode menus de VLC / masque d'opérations utilisateur, tous à zéro dans nos playlists). **Test (1) fait par l'utilisateur : Lecture > Chapitre > choisir un chapitre pendant la lecture saute bien au chapitre demandé** (libbluray accepte donc la recherche de chapitre en mode menus : structure des repères correcte). Les boutons précédent/suivant de la barre VLC restent donc très probablement des commandes de liste de lecture (elles relancent le disque) et non un défaut du disque ; les touches « chapitre suivant/précédent » d'une télécommande de lecteur matériel restent à vérifier sur matériel. Tests (2) « pas de menus de disque » et (3) journal VLC non faits (inutiles sauf si le doute persiste).
**Non vérifié** : le comportement des lecteurs matériels, la pagination (12 chapitres au plus).

**À ne pas croire sans vérification :**
- Les noms et opérandes du désassembleur de commandes, surtout `SET_BUTTON_PAGE`, `SET_STREAM`, `PLAY_PL_PM` (opérandes en champs de bits mal décodés : valeurs comme `PSR67108864`). Les `MOVE`, `CMP`, `GOTO`, `JUMP` sont plus fiables. À comparer avec `mobj_dump` de libbluray.
- La disposition binaire de l'EP_map (reconstituée de mémoire, mais valeurs cohérentes).
- Les attributs de flux du `.clpi` et du PlayItem, et la section ClipMark : gardés **bruts**.
- Les libellés de types de SubPath : seuls 2 (audio) et 3 (menu IG) sont nommés.
- Mon encodeur RLE : redonne les mêmes images que l'original pour les 93 objets (68 identiques à l'octet), mais jamais testé sur un vrai lecteur.
- Le `.m2ts` régénéré n'a **jamais été lu par un vrai lecteur ni par libbluray** : identique à la référence ne prouve pas que d'autres contenus (autre ICS, autre nombre d'objets) soient acceptés.
- Règle de placement du 2e PCR (paquet 863 dans la référence, 30 ms après le premier) : inconnue. La spécification limite l'intervalle à 100 ms, mais je n'ai pas vérifié ce qu'attendent les lecteurs pour un flux très court.
- Règle de l'ATS et durées de décodage : vérifiées sur ce seul fichier. L'écart d'ICS entre PTS du dernier ODS et PTS de l'ICS (environ 0,16 s ici) est un choix de multiAVCHD, pas une règle connue.
- Question ouverte : l'ICS annonce 1920×1080 alors que la vidéo est en 720p. Règle exacte sur la taille du plan graphique non confirmée.

### Erreurs déjà corrigées dans la conversation
- Version des fichiers : `0100` et non `0200` / `0300`.
- Objet 1 : il lance la playlist 0, pas la 253.
- Objets 255 et 259 : ce sont des objets de service de multiAVCHD (MovieObject de 264 objets), pas des anomalies.

### Règles de multiplexage retrouvées (implémentées dans `mux.py`, vérifiées sur `80002.m2ts`)
- Ordre des paquets : PAT, PMT, SIT (PID `0x1f`), 1 PCR, paquets du flux IG, bourrage.
- Bourrage final : paquets nuls (`47 1F FF 10` + 184 octets à 0) jusqu'à un multiple de **32 paquets sources** (6144 octets).
- **ATS** du paquet n = floor(n × 188 × 27 000 000 / débit), avec débit = `peak_rate` × 50 o/s, où `peak_rate` (ici 107 679, soit 5 383 950 o/s) est déclaré dans le descripteur de la SIT.
- **PCR** (PID `0x1001`, paquet en adaptation seule, compteur de continuité 0) : valeur = ATS du paquet qui le porte.
- PAT, PMT, SIT : un paquet chacun, bit de priorité à 1 dans l'en-tête TS (`47 60 ..`, `47 61 ..`), CRC32 MPEG (poly `0x04C11DB7`, init `0xFFFFFFFF`) recalculé.
- PMT : descripteurs `05 04 'HDMV'` et `88 04 0F FF FC FC`, puis le flux IG (type `0x91`, PID `0x1400`).
- PES IG : stream_id `0xBD`, un segment par PES, flags `0x85` + `0xC0` (PTS+DTS) ou `0x80` (PTS seul). Dernier paquet d'un PES : champ d'adaptation de bourrage (`00 FF FF ...`). Compteur de continuité par PID, incrémenté à chaque paquet avec charge utile.
- **Horodatage** (90 kHz) : durée de décodage d'un ODS = ceil(largeur × hauteur × 90000 / 8 000 000) ; DTS(ICS) = PTS(ICS) − ceil(1920 × 1080 × 90000 / 8 000 000) ; PTS(PDS) = DTS(ICS) ; DTS(ODS k) = PTS(ODS k−1) ; PTS(END) = PTS du dernier ODS.

### Tests VLC (Windows) : état
- `STREAM/00000.m2ts` seul se lit dans VLC : le film est sain.
- **Disque d'origine** : VLC l'ouvre comme Blu-ray (`bluray:///C:/.../AVCHD`), lit `index.bdmv` (« First play: 1, Top menu: 1, HDMV Titles: 1, BD-J Titles: 0 ») et le titre « multiAVCHD - GSC 0008 » (dossier `META`), puis libbluray s'arrête : « bluray: stopping playback after fatal error », en boucle, sans fenêtre d'erreur. Cohérent avec le démarrage vers la playlist 254, absente (échec de démarrage du titre).
- **Copie avec `bdmv_overlay.zip`** (notre `index.bdmv`, `MovieObject.bdmv`, `80002.m2ts`, `80002.clpi`, playlist identique à l'original) : **fonctionne** d'après l'utilisateur. Premier succès de bout en bout : la chaîne index → objet → playlist → clip générée par notre code est acceptée par libbluray / VLC. À préciser : le film démarre-t-il seulement, ou le menu pop-up s'affiche-t-il aussi ?
- Les variantes `out/test1_index_seul.zip` et `out/test4_sans_menu.zip` (`tests/build_bisect.py`) n'ont plus d'objet ; conservées au cas où une régression apparaîtrait.
- **Menu non visible dans VLC** sur la copie qui fonctionne : attendu pour un menu pop-up (`ui_model=1`), qui n'apparaît que sur demande ; on ignore comment déclencher le pop-up dans VLC. Variante `out/test_menu_affiche_d_office.zip` (`python -m tests.build_always_on`) : même clip avec `ui_model=0` (1 seul octet de différence), à copier sur la copie qui fonctionne (`BDMV/STREAM/80002.m2ts`) pour voir si les boutons s'affichent au démarrage. **Résultat : les 2 boutons s'affichent au démarrage par-dessus la vidéo dans VLC (constaté par l'utilisateur).** Notre flux IG (ICS, PDS, ODS, mux) est donc décodé et rendu par libbluray ; `ui_model=0` = menu affiché d'office, `ui_model=1` = pop-up (cohérent avec le rendu de la référence). **Navigation vérifiée dans VLC** : les flèches changent le bouton actif, Entrée sur « Lire le film » relance la vidéo (`JUMP_TITLE 1` exécuté). Reste à vérifier les lecteurs matériels.
- **Disque avec menu d'accueil (à tester)** : `python -m tests.build_disc_accueil` → `out/bdmv_overlay_accueil.zip`. Objet 0 (First Playback et Top Menu) : `PLAY_PL 1`, `NOP` ×3, `GOTO 0` (boucle, motif repris de l'objet 259 de multiAVCHD) ; objet 1 (titre 1) : `PLAY_PL 0` puis `JUMP_TITLE 0` (retour au menu à la fin du film). `00001.mpls` = playlist de menu (fond = le film en attendant un clip dédié, SubPath IG, identique à la playlist déjà validée) ; `00000.mpls` = film seul. Menu en `ui_model=0` ; boutons : « Lire le film » (`JUMP_TITLE 1`), « Menu principal » (`JUMP_TITLE 0`). **Résultat dans VLC : OK sur les 4 points** (menu affiché au démarrage, fond qui repart en boucle avec le menu, « Lire le film » lance le film sans boutons, retour au menu en fin de film). Reste à tester sur lecteur matériel.
- Pistes non utilisées : variable `BD_DEBUG_MASK` pour obtenir les messages de la machine virtuelle HDMV (non vérifiée) ; option VLC « menus de disque » à décocher.

### Fichiers tiers (`rle_menu.py`, `igs_muxer.py`, `hdmv_menu_builder.py`) : évalués puis remplacés
Testés contre nos parseurs validés, ils étaient inutilisables : types de segments faux (0x11 / 0x12 au lieu de 0x18 / 0x15), longueur sur 3 octets au lieu de 2 (END = `80 00 00 00`), ODS avec `sequence_flag` 0x80, RLE invalide, ICS à structure inventée (illisible par `Ics.parse`, aucune commande de navigation), palette Rec.601 pleine plage dans l'ordre Y/Cb/Cr (réf. : BT.709, 16–235, Y/Cr/Cb) et palette de « normal » réutilisée pour les 3 états, M2TS sans PAT/PMT/SIT/PCR, ATS +300 par paquet, mauvais préfixe de PTS, un seul PES (plante au-delà de 65 535 octets), `make_clip_clpi` appelé sur un clip de menu. **À supprimer du projet : `rle_menu.py` et `igs_muxer.py`** (remplacés par `ig.py`, `mux.py`, `png_palette.py`). `hdmv_menu_builder.py` est réécrit avec le même appel.

**Import de boutons PNG (fait, à tester dans VLC)** : `MenuButton(label, titre, x, y, images=(normal, sélectionné, activé))` (chemins ou images PIL ; 1 ou 2 images acceptées). Palette commune à tous les boutons : pixels à alpha < 16 → indice 0 ; sinon 63 couleurs (médiane coupée de Pillow) × 4 paliers d'alpha (255, 192, 128, 64), soit 140 entrées sur le test. Mesures sur des boutons à dégradé et bords adoucis (`tests/build_menu_png.py`) : écart moyen de couleur 1,82/255 (pire pixel 27), écart d'alpha 0,04/255, aucun pixel transparent mal rendu, trois états distincts, relecture de l'ICS et de la PDS identique. Voisins de boutons calculés d'après la géométrie (grille 2×2 vérifiée) ; un bouton qui sort de l'écran est refusé. Le menu texte par défaut est resté **identique à l'octet près** à celui validé dans VLC. Limites connues : images photographiques (dégradés fins) non testées, pas de tramage ; pas de fond d'écran PNG (il faut un clip vidéo : voir muxeur vidéo) ; `ics_pts` doit rester cohérent avec la playlist (42956 ↔ 21478). Livrable : `out/menu_png_overlay.zip` (3 fichiers à superposer à `disque_complet`). **Résultat dans VLC : fonctionne, y compris avec les propres PNG de l'utilisateur** (confirmé). Le projet est installé chez l'utilisateur dans `C:\Users\moyla\Desktop\Authoring\code` (archive `bdauthor_code.zip`).

## 4. Enseignements pour le générateur

- Un menu minimal n'a pas besoin de la mécanique de multiAVCHD : 2 ou 3 objets MovieObject suffisent (démarrage vers le menu, attente de clic, lecture du film), et un ICS d'une page avec quelques boutons.
- Le clip de menu est séparé du film, déclaré dans un SubPath de la playlist, et **sans EP_map**.
- Le menu doit contenir : PAT, PMT, 2 paquets PCR (PID de PCR distinct du flux IG), paquets IG avec PTS/DTS cohérents, bourrage.
- Cohérences à respecter entre fichiers : mêmes PID dans la table des flux de la playlist et le `.clpi` ; temps du SubPlayItem = séquence du `.clpi` du clip de menu ; temps du PlayItem = séquence du `.clpi` du film ; numéros de clips et de playlists référencés par les commandes.
- Ordre des segments dans le flux IG : ICS, PDS, ODS (un par PES), END.
- Dossiers facultatifs pour un disque HDMV minimal : `BDJO/`, `JAR/`, probablement `AUXDATA/` et `META/` (à confirmer sur lecteur).

## 5. Feuille de route (mise à jour après le multiplexeur de film)

0. **En attente de retour utilisateur** : lecture de `disque_perso.zip` dans VLC (image, son, synchronisation, menu d'accueil, retour au menu en fin de film).
1. ~~Disque multi-titres~~ : construit, vérifié, validé dans VLC (menu, sélection, lancement, retour au menu, menu fixe). Reste : pagination au-delà de 12 vidéos, mode sans réencodage, chapitres internes à une vidéo.
2. ~~Fond de menu dédié~~ (image fixe, fait dans `project.py` ; reste : fond animé / boucle vidéo, option de répétition)  : clip propre (réutilise encode + vmux), playlist de menu qui ne dépend plus du film ; piste : mode « still » de la PlayItem (non vérifié).
3. ~~Chapitres~~ (validés dans VLC, hors liste de chapitres de VLC et touches suivant/précédent, non encore testées) :  marques dans la playlist (`Mark`) + menu de chapitres (commandes `PLAY_PL_PM` à opérandes immédiats : mot d'instruction à relever et tester ; `SET_BUTTON_PAGE` pour la pagination : désassemblage actuel peu fiable).
4. **Voie de test matériel** : image ISO UDF 2.50 (outil externe type ImgBurn, ou écriture maison) gravée sur BD-R/BD-RE, ou lecteur multimédia lisant les dossiers BDMV avec menus. Décision à prendre avec l'utilisateur (matériel disponible ?).
5. **Passage à l'échelle** : lecture/écriture en flux (le code lit tout en mémoire), ffmpeg en tube, avance de PCR/ATS sans second passage, EP_map en flux ; simulation du modèle de tampon T-STD.
6. **Autres formats** : 1080p24 / 1080i (code écrit, jamais exécuté), audio multiple et sous-titres PG (segments PCS/WDS, similaires à l'IG).
7. **Application** : ligne de commande pilotée par un fichier de projet, puis interface (Tauri), puis portage Rust par couches (les tests aller-retour et les rejeux de la référence servent de valeurs d'or).

Suite recommandée (non commencée) : **pagination des menus** (plus de 12 vidéos ou de 12 chapitres) avec le motif déjà validé — une playlist + un clip IG par page, boutons « Page suivante / précédente » par `JUMP_TITLE` vers des titres de page, sans `SET_BUTTON_PAGE` ; puis voie de test matériel (image ISO UDF 2.50), puis flux pour les longs films, puis mode sans réencodage.

## 6. Pour reprendre

Joindre ce fichier et le dossier `bdauthor/`, puis préciser l'étape visée (par défaut : le menu minimal généré de zéro, étape 1 ci-dessus). Si une nouvelle conversation n'a pas accès aux fichiers de référence, redemander `80002.m2ts` (172 Ko) pour les tests.


## Pagination des menus (fait)
- `project.paginate(n, par_page)` : <= 12 elements = une page (sortie identique octet pour octet a la version validee VLC, verifie) ; sinon pages equilibrees de `menu.par_page` (defaut 8).
- Chaque page = clip IG + playlist (fond partage) + titre. Page 1 du menu principal = Top Menu (clip 80002, playlist N) ; pages suivantes : titres N+1.. ; puis, par video a chapitres, ses pages de chapitres et un titre « lecture depuis repere » (`nav.plan_navigation`, `make_movie_objects_paged`).
- Boutons « < Precedent » / « Suivant > » (barre du bas, emplacements fixes) ; chapitres multi-pages : Precedent / Tout lire / Retour / Suivant.
- Apres une video, retour a sa page du menu principal (ou a son menu de chapitres) ; « Retour » des chapitres ramene a la page contenant la video.
- Limites : 99 videos, 99 chapitres/video. Test : `python -m tests.build_pagination video.mp4 14` (14 videos, 17 chapitres sur 3 pages) -> checker OK, 21/21 titres atteignables. VALIDE dans VLC (12 videos reelles, par_page 5 : 3 pages de menu, 10 chapitres sur 2 pages) : navigation entre pages, retour sur la bonne page.

- Cache d'encodage (`.cache_bdauthor`, `menu.cache`) : une video deja encodee n'est pas refaite ; un build interrompu reprend. Utile quand l'environnement limite la duree d'une commande.


## Rust (en cours) : `rust/bdauthor-core`
Decision : on ne reecrit pas ffmpeg/x264 ; le programme final PILOTE ffmpeg (ffmpeg.exe/ffprobe.exe livres a cote de l'executable, ou dans le PATH, ou `BDAUTHOR_FFMPEG_DIR`). Construire : `cargo build --release` dans `rust/bdauthor-core` ; tests : `cargo test`. Aucune dependance externe (compile hors ligne).
- Fait : `src/ffmpeg.rs` = port de `encode.py` : `Tools::locate`, `probe` (ffprobe `-of flat`), `pick_target`, `encode_to_ts` (progression `-progress pipe:1`, annulation, erreurs lisibles), `make_still_ts`, `extract_thumbnail`. CLI de test : `bdauthor-ffmpeg probe|encode|still|thumb`.
- Valide : le TS produit par Rust est IDENTIQUE octet pour octet a celui de Python (meme sha1, Python deterministe sur 2 runs) ; tests unitaires (cibles, sondage, arguments) + tests d'integration (bout en bout, annulation, fichier invalide, source sans audio).
- Windows : `cargo test`, `cargo build --release` et `bdauthor-ffmpeg probe` valides par l'utilisateur (Rust + MSVC installes ; ffmpeg 9.0.2 essentials dans `code/ffmpeg-9.0.2-essentials_build/bin`, indique via `BDAUTHOR_FFMPEG_DIR`). L'environnement de Claude ne peut PAS lancer cargo sous Windows (shell Linux) : l'utilisateur lance les commandes et colle les sorties.
- A revoir : source 25 i/s (PAL) -> actuellement 720p24 (suppression d'images) ; 720p50 ou 1080i50 serait plus fidele (meme comportement dans encode.py). Source sans piste audio : echec ffmpeg (`-map 0:a:0`), a gerer (piste silencieuse ou pas d'audio).
- Suite : porter `m2ts/tsclip/vmux/mux/ig/menu/nav/check` en Rust, module par module (round-trip + comparaison octet pour octet avec la sortie Python), puis l'interface egui.

### Portage Rust, etape 1 (fichiers de navigation) : FAIT
- Modules : `reader`, `index`, `mobj` (+ desassembleur), `mpls`, `clpi` (EP_map comprise), `nav` (make_index, make_playlist, plan_navigation, make_movie_objects_paged). Outil : `bdinspect <fichier> [--verifier]`.
- Validation : (1) aller-retour octet pour octet sur 13 fichiers (references multiAVCHD + fichiers generes) ; (2) descriptions textuelles identiques a `dump()` de Python (golden `*.dump`) ; (3) fichiers construits par Rust identiques a ceux de Python (index, 4 playlists dont la playlist par defaut == 00000.mpls de multiAVCHD, MovieObject pagine == celui du disque de test valide dans VLC) ; (4) troncatures : erreur, jamais de panique. 10 tests au total (`cargo test`).
- Prochaine etape : `m2ts` (paquets TS, PES, segments), `tsclip` (scan_clip, clpi de clip), `vmux`/`mux` (muxage film et IG), puis `ig`/`menu`/`png_palette` (RLE, ICS/PDS/ODS), `check`, et enfin le programme complet (`project`).

### Portage Rust, etape 2 (flux video et muxage) : FAIT
- Modules : `m2ts` (paquets TS paresseux, PES, segments), `mux` (PAT/PMT/SIT, PCR, PES IG, `ig_pes_specs`, `mux_ig_clip`), `vmux` (`mux_film`), `tsclip` (PSI, SPS H.264, `scan_clip`, EP_map, `make_clip_clpi`), `film` (`load_encoded`, AC3, descripteurs, `make_film_m2ts`, `make_still_m2ts` = ffmpeg + muxage). CLI : `bdauthor-ffmpeg film <src> <sortie.m2ts> [crf] [preset] [langue]` (ecrit aussi le .clpi).
- Les calculs d'horloge reprennent les memes operations flottantes que Python, dans le meme ordre (f64 IEEE), et la division flottante de Python (`//`) est reproduite : la sortie est identique a l'octet.
- Valide : (1) muxage synthetique (dont cas avance augmentee, PES > 64 Kio) == Python ; (2) rejeu du menu de reference 80002.m2ts == original (PCR aux positions de reference) et politique par defaut == Python ; (3) `ig_pes_specs` recalcule les PTS/DTS de la reference ; (4) film de reference remuxe == Python et `.clpi` regenere == 00000.clpi de multiAVCHD ; (5) TS ffmpeg -> m2ts + clpi == Python ; (6) bout en bout MP4 -> m2ts + clpi par Rust == Python, octet pour octet (9,5 Mo). Test du film de reference : variable `BDAUTHOR_REF_M2TS=<chemin de 00000.m2ts>` (sinon ignore).
- Limites connues (heritees de Python) : fichiers lus entierement en memoire ; 720p59.94 seul teste sur materiel/VLC.
- Prochaine etape : `ig`/`menu`/`png_palette` (RLE, ICS/PDS/ODS, quantification des PNG avec la crate `image`, polices), puis `check` et le programme complet (`project`).

## Rust, etape 3 : menus IG (en cours)

Decoupage en deux crates pour garder le coeur sans dependance (testable hors ligne) :
- `bdauthor-core` (std seulement) : `ig.rs` (ICS/PDS/ODS, RLE), `palette.rs` (coupe mediane + Floyd-Steinberg maison,
  remplace Pillow), `menu.rs` (MenuButton, auto_neighbors, build_menu, build_menu_clip, make_clpi, required_ics_pts).
  Teste dans le conteneur : aller-retour parse/pack sur l'IG de reference, voisins / CLPI / PTS identiques a Python,
  clip genere relu par le parseur Python.
- `bdauthor-render` (dependances `image` 0.25 + `ab_glyph` 0.2) : chargement PNG, redimensionnement Lanczos, texte,
  rectangles arrondis. **Non compile dans le conteneur** : a valider par `cargo test` sous Windows.
Difference connue : le codeur RLE de reference code les series de 3 pixels en litteral, le notre (comme Python) a partir de 3 en run ;
les deux sont valides, les pixels decodes sont identiques. La quantification n'est plus octet pour octet celle de Pillow.
A faire ensuite : `check`, `project` (JSON, pagination, cache), `disc`, puis l'interface egui.

### Rust : `check` (verificateur) porte
`bdauthor-core::check` + binaire `bdcheck <BDMV>` : sortie identique a `python -m bdauthor.check` sur les disques
chapitres et pagination ; sur un disque corrompu, Rust signale les erreurs la ou Python plante (m2ts tronque).

### Rust, etape 4 : constructeur de projet (`project`) porte
`bdauthor_core::project::build_project` : JSON -> encodage (cache `.cache_bdauthor/*.m2ts`) -> fond -> menus principaux
pagines -> menus de chapitres -> playlists/navigation -> ecriture BDMV + verification. Le dessin passe par le trait `Renderer`
(implemente par `bdauthor_render::FontRenderer`). Nouveaux modules sans dependance : `json`, `png` (ecriture), `disc`.
Valide dans le conteneur sur un projet de 3 videos (pagination 2 pages, 3 chapitres sur 2 pages) : index, MovieObject, playlists,
clpi et m2ts des videos octet pour octet identiques a la version Python ; seuls les clips de menu/fond different (dessin).
Programme complet en ligne de commande : `bdauthor-build projet.json dossier_sortie` (crate bdauthor-render).
Limite connue : le cache ne conserve pas les statistiques d'encodage (juste le m2ts).

### Rust, etape 5 : interface graphique
`bdauthor-core::model` (modele de projet editable, lecture/ecriture JSON en conservant les champs inconnus, validation) — teste ici.
`bdauthor-gui` (egui/eframe 0.29 + rfd 0.15) : liste de videos (ajout, ordre, retrait), titre/vignette/chapitres par video, reglages
disque et menu, construction dans un thread avec progression, journal, annulation, rapport de verification. Non compile dans le conteneur.

## Editeur visuel de mise en page (export 8)
- Onglet « Mise en page » de la GUI : liste des pages (menu principal + menus de chapitres, chaque page separement),
  apercu fidele (meme code de dessin que la construction), glisser / redimensionner (coins), fleches (Maj = 10 px), Suppr,
  inspecteur (x/y/w/h, texte, couleur, alignement, action des boutons, image), ordre des vignettes, superposition.
- Une page sans personnalisation garde la grille automatique (identique a avant). « Personnaliser » part de cette grille.
- JSON : "mise_en_page": {"principal": {"<page>": {"elements": [...]}}, "chapitres": {"<video>": {"<page>": {...}}}} (numeros a partir de 1).
- Code : core/design.rs (modele + validation), core/editor.rs (logique testee), render (apercu), gui/editor_ui.rs (egui).
- Teste ici : tout le coeur (cargo test). NON compile : render (modifs apres export 7) et gui (eframe indisponible hors ligne).

## Depot git (a maintenir)
- Depot local dans `code/` (branche `main`, identite locale moyla). `.gitignore` exclut target/, caches, videos, sorties, ffmpeg, archives.
- Regle : apres chaque lot de modifications livre (et valide ou signale comme non compile), faire un commit descriptif
  (`git add -A && git commit`), via `device_bash` dans `$HOME/mnt/Authoring/code` (demander d'abord `device_request_delete_permission`
  pour le dossier connecte : git cree/supprime des fichiers .lock). Terminer les messages par les lignes d'attribution demandees par la session.
- Ne jamais committer de gros fichiers (verifier `git status` avant `git add -A`).

## Plan (superposition) et contenu des vignettes
- L'ordre de la liste d'elements d'une page = ordre de superposition (le dernier est au premier plan), tous types confondus.
  Fond : textes/images dans l'ordre. Boutons (IG) toujours au-dessus du fond ; un texte/image place devant un bouton est
  pose sur les images du bouton (`Renderer::overlay`, seulement sur l'intersection avec le cadre du decor).
- Vignette : champ `assign` (JSON `element`, 1-based) = element de la page montre ; sinon automatique. `slot_assignment`
  (valide, sans doublon, le reste dans l'ordre) et `assign_slot` (echange avec la vignette qui avait deja cet element).
- Editeur : boutons Tout derriere / Derriere / Devant / Tout devant, menu « Contenu » d'une vignette, liste des elements.
- `copy_decor` conserve l'ordre de superposition de la page source.

## Pages libres (menu principal)
- `Designs.free` (JSON `mise_en_page.libres`, liste) : pages ajoutees apres les pages automatiques. Leurs vignettes ont un `assign`
  OBLIGATOIRE = indice de video (JSON `element`, 1-based) ; doublons permis ; une vignette sur une video a chapitres ouvre son menu de chapitres.
- Navigation : pages automatiques + libres forment une seule suite (Precedent/Suivant sur `total_main`). Titres/playlists/IG
  numerotes comme les pages 2.. ; chaque page libre a son propre fond (`fond_libreN`).
- `add_free_page` (editor.rs) ajoute une page vierge (bouton Precedent seul) et un bouton Suivant a la page precedente si elle est
  personnalisee ou libre et n'en a pas ; une page automatique non personnalisee l'obtient de la grille.
- `swap_videos` / `remove_video` remappent (ou retirent) les vignettes des pages libres. `ProjectModel::validate` refuse les pages libres invalides.
- GUI : bouton « + Page libre » (liste des pages), « Supprimer cette page », menu « Vidéo » par vignette. Non compile (eframe hors ligne).

## Boutons « Aller à une page »
- `NavAction::Goto` + `Kind::Nav.target: Option<Target>` ; `Target::Main(p)` (pages automatiques puis libres, 0-based) ou
  `Target::Chapters(video, page)`. JSON : `"action": "aller_a", "cible": {"menu":"principal","page":2}` ou
  `{"menu":"chapitres","video":1,"page":1}` (1-based). Toujours applicable (meme sur la page visee).
- Construction : titre vise = `plan.main_titles[p]` ou `plan.chapters[v].pages[q]` ; cible absente/inexistante => erreur avant encodage
  des menus (message avec le texte du bouton). `ProjectModel::validate` fait une verification partielle (hors chapitres automatiques).
- Coherence : echange/retrait de video et suppression d'une page libre remappent ou annulent les cibles (`Designs::page_removed`).
- GUI : action « Aller à une page » + menu « Page cible » (toutes les pages), avertissement si la page n'existe plus.

## Sous-titres PGS (Blu-ray, images)
- Par video : `"soustitres": [ {"fichier":"x.srt"|"piste":N, "langue":"fra", "decalage_ms":0, "style":{police,taille,couleur,contour_couleur,contour,position(bas|milieu|haut),marge}} ]`, 8 pistes au maximum. Tailles du style donnees pour 1080 lignes, mises a l'echelle pour 720.
- Modules : `subtitle.rs` (SRT/ASS/UTF-8/UTF-16/cp1252, fusion des chevauchements, reglages), `pgs.rs` (segments PCS/WDS/PDS/ODS(fragmentes)/END, ensembles d'affichage, relecture pour tests), `film.rs` (`encode_sub_tracks`, `mux_encoded_subs`, `load_encoded_m2ts`), `vmux.rs` (`mux_film_subs`, `SubTrack`; sortie identique octet pour octet sans sous-titres), `tsclip.rs` (`make_clip_clpi_pg`, `ClipScan.pg`), `nav.rs` (`PlaylistParams.pg`, entrees PG du STN), `check.rs` (controle des flux PG), `ffmpeg.rs` (`list_subtitle_streams`, `extract_subtitle_srt`), `render` (`FontRenderer::subtitle` avec contour), GUI `subs_ui.rs`.
- Principe : le cache garde le clip encode SANS sous-titres ; les sous-titres sont ajoutes a chaque construction par remuxage (rapide). PID 0x1200+n, stream_type 0x90, PES stream_id 0xBD.
- Chaque replique = une epoque (PCS 0x80 + WDS + PDS + ODS + END) puis un ensemble d'effacement (PCS normal sans objet) ; si la replique suivante arrive avant que l'effacement soit possible (delai de decodage ~0,26 s en 1080), elle remplace directement la precedente. Duree minimale 0,3 s.
- Valide : ffmpeg decode les flux et affiche aux bons instants et positions (tests/subtitle_e2e.rs). NON valide : lecteur de salon, VLC (a tester par l'utilisateur).
- Non fait : piste « par defaut » (SET_STREAM / PSR2), sous-titres images integres (PGS/DVD), styles ASS (seuls texte et sauts de ligne sont conserves), italique/gras.

## Textes et images cliquables, bouton vers une video
- `Target::Video(v)` (JSON `"cible": {"lire":"video","video":N}`) : un bouton « Aller a » peut lire n'importe quelle video (titre v+1, retour comme apres une vignette).
- `Kind::Text/Image { .., link: Option<Target> }` (JSON : champ `cible` sur l'element) : l'element devient un bouton IG. Etat normal = l'element seul sur fond transparent, selectionne = cadre bleu translucide borde de blanc (`Renderer::decor_button`, defaut = bouton texte vide). Il n'est plus dessine dans le fond.
- `Item::link()`, `Item::targets_mut()` ; `is_button()` inclut les elements cliquables (chevauchements, debordement) ; echange/retrait de videos et suppression de pages renumerotent aussi ces liens.
- Corrige au passage : les textes/images poses sur les boutons (superposition) etaient a l'echelle de la video et non du menu quand le menu est en 1080 sur une video 720.
- GUI : case « Cliquable » + liste « Destination » (pages puis « Lire la video N ») dans l'inspecteur ; la liste du bouton « Aller a » propose aussi les videos.

## Vignettes : n'importe quelle video, destination au clic
- `Kind::Slot { thumb_h, assign, video, link }`. `video: Some(v)` = la vignette montre la video v du projet sur n'importe quelle page et ne compte pas parmi les elements de la page (`slots()`/`slot_count()` = vignettes d'elements seulement ; `all_slot_count()` pour la limite par page). `link: Some(t)` remplace le comportement habituel (chapitres sinon lecture ; sur une page de chapitres : lecture depuis le chapitre).
- JSON : `"video": N` (1-based) et `"cible": {...}` sur la vignette. Pages libres : leurs vignettes utilisent maintenant `video` ; un ancien `"element"` d'une page libre est converti a la lecture.
- Construction : `page_buttons` recoit `video_specs` (une SlotSpec par video) ; les pages libres n'ont plus d'elements propres. Echange/retrait de video renumerotent ou suppriment ces vignettes sur toutes les pages.
- GUI : liste « Contenu » (elements de la page puis toutes les videos) et « Au clic » (habituel, lire la video associee, pages, autres videos) ; l'apercu recoit les vignettes dans l'ordre de la liste. Sur le canevas : « V3 » = video 3 choisie, « → » = destination personnalisee.

## Vignettes sans video, doublons, images SVG
- `Kind::Slot.video: Option<Option<usize>>` : `None` = vignette d'element ; `Some(Some(v))` = video v ; `Some(None)` = vignette de video sans video (apres retrait de la video, ou pas encore choisie) : conservee dans la mise en page, avertissement, absente du disque. JSON : `"video": null`.
- Retrait d'une video : ses vignettes restent en place, sans video (plus de suppression).
- Doublons : plusieurs vignettes peuvent montrer le meme element de la page (plus d'echange automatique) ou la meme video. `slot_assignment` renvoie `usize::MAX` pour une vignette automatique sans element restant (erreur « sans élément ») ; un element sans vignette n'est plus qu'un avertissement (`unshown_elements`).
- SVG : `bdauthor-render` depend de `resvg = "0.45"` (`load_svg`, `is_svg`, rasterise a 2048 px sur le plus grand cote puis reduit au cadre ; polices systeme pour les textes du SVG). `load_image` accepte SVG/SVGZ partout (images, fond). Fond de menu commun en SVG : converti en PNG par le moteur de dessin avant ffmpeg. `image` gagne la fonction `bmp` (le BMP etait propose mais non lisible).
- API resvg ecrite sans pouvoir compiler (crates.io inaccessible depuis le cloud) : verifier a la premiere compilation Windows.

## Apparence des boutons (surbrillance), etat active, images dessinees, poids des pages
- `style.rs` : `ButtonStyle` (fond/texte/cadre normal, assombrissement, fond/texte/cadre selectionne, arrondi, halo + couleur, agrandissement %, fond translucide et soulignement des textes/images cliquables, etat active distinct + couleurs), `Skin` (images normal/selectionne/active), `Look` (par element), `Weight`, `DECODED_LIMIT` = 16 Mio (hypothese sur la spec IG, a confirmer), `DECODE_RATE` = 8 Mo/s.
- JSON : `mise_en_page.style_boutons` (style du projet, enregistre seulement s'il differe du defaut) ; par element `"style": {...}` (style complet qui remplace celui du projet) et `"images_etats": {"normal","selectionne","active"}`.
- Trait `Renderer` : `thumb_button`/`text_button`/`decor_button` recoivent le style (deja a l'echelle) et rendent 3 etats ; nouvelles `skin_button` ; `overlay` sur 3 etats. Chaque image fait (w+2mx)x(h+2my) avec `style.margins(w,h)` = halo + zoom/2 ; le constructeur recentre une image de mauvaise taille, place le bouton en (x-mx, y-my) et rogne aux bords du menu ; l'etat active partage l'image selectionnee (meme Arc) s'il n'est pas distinct.
- `page_buttons` prend un `ButtonEnv` ; avertissement dans le journal si une page depasse `DECODED_LIMIT`.
- Rendu (`bdauthor-render`) : `finish_state` (arrondi, cadre, assombrissement, agrandissement, halo = silhouette floutee), `decor_face`, `skin_button`. Apercu : `render_page_preview(.., style, selected)` montre l'element choisi a l'etat selectionne.
- GUI : bouton « Style des boutons… » (fenetre, style du projet), inspecteur « Apparence propre a cet element » + images dessinees (navigation, textes/images cliquables), ligne « Poids des boutons » sous l'apercu (taille du menu deduite de la 1re video : 1080 si hauteur > 800 et <= 25 i/s, sinon 720 ; 1080 si l'option est cochee).
- Les chevauchements dus au halo/agrandissement ne sont pas encore signales par la validation.

## Simulation de la telecommande (editeur)
- Cœur (`editor.rs`) : `sim_buttons(info, design, style, n_videos, has_chapters)` -> boutons de la page dans l'ordre du disque (le 1er est selectionne a l'ouverture), cadre elargi des marges du style, `SimAction` (Go(Target), Play, PlayChapter, Prev, Next, Back) et voisins calcules par `menu::auto_neighbors` (meme regle que le disque). `page_of_target`, `main_page_of_video`.
- Rendu : `render_page_preview(.., selected, activated)` montre l'etat active de l'element choisi.
- GUI `sim_ui.rs` : bouton « ▶ Simuler la télécommande » ; fleches = voisins, Entree/Espace/clic = valider (etat active 350 ms) puis action ; lecture de video simulee par un ecran noir, retour comme sur le disque (menu de chapitres si « revenir aux chapitres » et chapitres, sinon page de la video) ; Echap quitte. Pendant la simulation, l'edition et l'inspecteur sont desactives ; le focus clavier des widgets est rendu chaque image.

## Accueil, ouverture, « Tout lire », formats PAL/entrelaces, sans son, fond anime et musique
- Accueil : `Designs.home: Option<Target>` (JSON `mise_en_page.accueil`, pages seulement). Si l'accueil n'est pas la page 1 : la page 1 recoit un titre ajoute (apres ceux du plan, `plan.main_titles[0]` remappe) et l'objet 0 (Top Menu) devient `JUMP_TITLE accueil`. Renumerote a l'echange/retrait de videos et a la suppression de pages.
- Ouverture : `menu.intro` (video) -> clip + playlist, objet dedie `PLAY_PL intro ; JUMP_TITLE 0`, index.bdmv First Playback = cet objet.
- « Tout lire » depuis le menu principal (`nav_applicable(PlayAll)` = vrai partout) : `make_playlist_multi` (un PlayItem par video, une marque au debut de chacune + ses chapitres) ; titre ajoute `PLAY_PL ; JUMP_TITLE 0`. `check.rs` controle les marques par PlayItem.
- Sans titre ajoute ni ouverture, MovieObject/index sont identiques a avant.
- Formats : `pick_target_scan` + `Scan` (Progressive / PsF / Tff / Bff) ; 1080p25 et 1080p29,97 -> 1080i en PsF (x264 fake-interlaced + pic-struct) ; 1080i50/59,94 conserves en vrai entrelace (tff/bff, ildct/ilme, scale interl=1) ; SD entrelace -> 720p50/59,94 desentrelace (bwdif) ; 25p SD -> 720p50 a cadence doublee (avant : 720p24). Fonds fixes en 1080 a 25/29,97 -> PsF (`scan_for`). `VideoInfo.field_order`. Cle de cache : `|cible=...` pour les nouvelles cibles seulement.
- ICS : `MenuOptions.frame_rate_code` (cadence du fond) au lieu de 0x10 fixe.
- Sans son : piste muette (anullsrc) ajoutee a l'encodage.
- Fond anime/sonore : `menu.fond_video` (video, 2 min max), `menu.son_fond_video` (garder son son), `menu.musique` (5 min max sur image fixe) ; `ffmpeg::make_menu_bg_ts`, `film::make_menu_bg_m2ts`, `project::menu_bg_clip` (cache `.cache_bdauthor/fond_*.m2ts`), `Renderer::compose_layer` (calque transparent des textes/images pose sur la video). Playlists des menus avec flux audio ; boucle forcee (still_mode 0).
- GUI : Projet (fond video, son, musique, ouverture), Mise en page (« Definir comme page d'accueil », repere « (accueil) »), apercu avec une image de la video de fond ; simulation : M / Origine = TOP MENU, « Tout lire » du menu principal.
- Tests : tests/formats.rs (PsF, 1080i50, sans son, fond fixe PsF), project.rs (accueil/ouverture/tout lire, fond anime + musique + cache).

## Fond propre a chaque page
- `PageDesign.bg: PageBg { visual: PageVisual (Project | Image(f) | Video(f, son)), music: PageMusic (Project | Silent | File(f)) }` ; JSON de la page : `"fond_page": {"image"|"video", "son_video", "musique": chemin | null (aucune)}` (absent = comme le projet). Repris par « copier le decor ».
- Construction : `page_bg` (fermeture de build_project) -> MenuBg + image de base propres a la page ; la page a alors toujours son propre clip (`own_background`), et `add_menu_page(.., looping)` met la boucle si ce fond est anime ou sonore. Pages automatiques (non personnalisees) : fond du projet.
- GUI : bouton « Fond de cette page… » (pages personnalisees), fenetre `page_bg_ui` ; l'apercu montre l'image ou une image de la video propre a la page (`page_visual`).
- Test : project.rs `per_page_backgrounds` (video propre sans musique, image propre + musique du projet, page commune).

## Fonds animes : plus de limite de duree
- Les limites (video 2 min, musique 5 min) etaient un choix de bdauthor, pas une regle Blu-ray : retirees. La video ou la musique est utilisee en entier (au moins 1 s).
- Journal : « fonds de menu animes ou sonores : N clip(s), X Mo au total ».
- GUI : duree et place estimee (60 Mo/min en 1080, 30 en 720 ; ~2,5 Mo/min pour image + musique) sous les reglages du fond du projet et dans « Fond de cette page » ; job `ProbeAudio` (ffmpeg::probe_duration) pour la duree des musiques.

## Bouton « Revenir a la grille automatique » retire
- A la demande de l'utilisateur : une page personnalisee le reste (Ctrl+Z reste possible juste apres la personnalisation). « Supprimer cette page » est conserve pour les pages libres. `PageInfo::remove_design` reste dans le coeur (pages libres, historique).

## Mise en page invalide : construction bloquee
- Plus de repli silencieux sur la grille automatique : `resolve_design` arrete la construction (`layout_failure` : chaque erreur dans le journal, prefixe « ERREUR de mise en page », puis erreur globale).
- Menu principal et pages libres verifies avant tout encodage ; menus de chapitres a leur construction (le nombre de chapitres depend des videos encodees).
- Tests : custom_layout_project (page de chapitres invalide -> arret, puis succes sans elle), invalid_main_layout_stops_before_encoding.

## Pages generiques (fin des pages automatiques et des menus de chapitres)
- Un menu = une liste de pages generiques, toutes personnalisees (JSON `mise_en_page.pages`, `Designs.generic`). Plus de grille
  automatique ni de « menu de chapitres » : une vignette montre une video choisie (`video`), lue depuis le debut ou, pour une
  vignette de chapitre, depuis `depart_s` ; options `libelle`, `image_s` (instant de la miniature), `retour` (absent = page
  d'origine, "accueil", ou {"page": n}).
- Conversion automatique des anciens projets (`convert::generate_pages`) : pages du menu principal (personnalisees ou grille),
  pages libres, puis pages de chapitres de chaque video (vignettes de chapitre, « Tout lire » -> lire la video, « Retour » -> page
  de la video). Faite par le constructeur (avec les chapitres reels) et par l'editeur a l'ouverture de « Mise en page »
  (`editor::convert_to_generic`, attend la duree des videos a chapitres automatiques). Un ancien menu de chapitres invalide bloque
  toujours la construction (verifie avant conversion).
- Disque : titres 1..n videos, n+1..n+P pages, puis un titre « lecture depuis un repere » par video ayant des vignettes de chapitre
  (MOVE r10<-r0 ; r0<-0 ; PLAY_PL_PM v, r10 ; JUMP_TITLE r1), puis « Tout lire ». Le registre r1 porte le titre de retour (mis par
  chaque bouton qui lance une lecture) ; les videos finissent par JUMP_TITLE r1. Reperes = chapitres de la video + departs des
  vignettes, cales sur l'image d'acces precedente.
- Editeur : « + Page », « + Pages generees » (grille + chapitres ajoutes a la fin), nom, monter/descendre (`swap_pages`, liens
  suivis), supprimer ; « + Vignette » video ou chapitre ; inspecteur : video, depart m:ss, libelle, image, au clic, apres la lecture.
  Simulation : `SimAction::PlayFrom`, retour par vignette. Option « revenir au menu de chapitres » retiree ; `par_page` ne sert plus
  qu'aux pages generees.
- Tests : tests/generic.rs (conversion, edition, simulation), project.rs `generic_pages_with_chapter_thumbnails` (registres,
  reperes, commandes des boutons). Interface non compilee dans le conteneur.

## Calques de l'editeur
- `PageDesign.layers: Vec<Layer {id, name, hidden, locked}>` (arriere -> premier plan, vide = liste simple) et `Item.layer` (id).
  JSON : page `calques: [{id, nom, masque?, verrouille?}]`, element `calque: id`. Les elements d'un calque sont consecutifs
  (`normalize_layers`, appele a la lecture et apres chaque operation) : l'ordre de la liste reste l'ordre de superposition sur
  le disque. Masque / verrouille : editeur seulement (apercu, selection a la souris) ; le disque n'en depend pas.
- `editor` : `insert_item` (premier plan d'un calque), `add_layer` (le 1er cree « Calque 1 » avec tout), `remove_layer`
  (elements gardes au calque voisin ou supprimes), `move_layer`, `move_item` (glisser-deposer), `hit_test_where` ;
  `to_front` / `to_back` / `move_in_list` restent dans le calque.
- Interface : liste « Calques et elements » (glisser-deposer d'elements et de calques, calque actif, double-clic / clic droit
  pour renommer ou supprimer, oeil, cadenas), choix du calque dans l'inspecteur, duplication juste devant l'original.
  Tests : tests/layers.rs. Interface non compilee dans le conteneur.

## Arborescence unique pages / calques / elements
- Panneau de gauche « Pages et calques » : pages (☰ glisser pour reordonner, double-clic ou ✏ renommer, 🗑 supprimer, ⌂ accueil),
  et sous la page affichee ses calques et elements (meme glisser-deposer qu'avant). Un element ou un calque depose sur le nom
  d'une autre page y est deplace (Ctrl : copie). Le panneau de droite ne garde que l'inspecteur.
- Coeur : `editor::move_page` (liens, retours, accueil suivent), `transfer_item`, `transfer_layer` (tests/layers.rs).

## Sous-titres choisis par un bouton
- `Item.sub: SubChoice { Keep, Off, Track(k) }` (JSON element `soustitres` : "aucun" ou numero de piste 1-based ; absent = garder).
  Le constructeur ajoute en tete des commandes du bouton `SET_STREAM` (0x51C0_0001, operandes immediats) : dst = 0xC000 | piste
  (piste PG valide + affichage) ou 0 (affichage coupe, piste inchangee) — PSR2, conserve pour les lectures suivantes.
- `Item::played_video()` : video lancee par le clic (Aller a / lien vers une video, vignette). Inspecteur : liste « Sous-titres »
  des pistes de cette video ; validation du modele si la piste n'existe pas ; simulation : rappel dans le texte de lecture.
- Le reglage des sous-titres ne depend plus de la destination : liste « Sous-titres » sur tout element cliquable (pistes du
  projet par numero, langues rencontrees). Nouvelle destination `Target::Stay` (JSON `{"rester": true}`, « Aucune : rester sur
  cette page ») : commandes du bouton = [SET_STREAM, NOP], le menu reste affiche.

## Langues de sous-titres du projet
- Liste `langues: [{id, nom, code}]` (modele `ProjectModel.languages`, `subtitle::Language`) : l'identifiant est stable ; le RANG dans la
  liste est le numero de piste PG sur le disque, identique dans toutes les videos (max 8). Chaque piste d'une video a `langue_id`
  (son code `langue` suit celui de la langue). Bouton : `SubChoice::Lang(id)` (JSON `soustitres: {"langue": id}`) -> SET_STREAM rang.
- Generation (`project.rs` : `slot_tracks`, `add_subtitles`) : avec une liste de langues, TOUTES les videos recoivent les memes
  pistes, de la premiere langue a la derniere utilisee ; une langue absente d'une video = piste vide (un ensemble d'affichage sans
  objet, `pgs::empty_track`, car le controleur refuse un flux declare sans paquet). Aucun registre ni commande par video : marche
  aussi avec « Tout lire » (une seule playlist). Sans liste de langues : ancien fonctionnement (pistes dans l'ordre, `Track(k)`).
- `ProjectModel::ensure_languages` (appelee a l'ouverture dans l'interface) deduit la liste des anciens projets : une langue par code
  de piste, pistes rattachees, boutons `Track(k)` convertis en `Lang`. Validation : langue non choisie, langue en double dans une
  video, langue sans sous-titres dans aucune video, langue supprimee.
- Interface : fenetre « Langues des sous-titres » (menu Edition : ajouter, renommer, code, monter/descendre, retirer),
  liste « Langue » par piste, liste « Sous-titres » du bouton = langues du projet. Tests : tests/languages.rs, tests/project.rs
  (`language_list_fixes_track_numbers`). Non valide sur lecteur reel (flux PG vides, SET_STREAM).

## Pistes audio par langue
- Meme liste de langues que les sous-titres (fenetre Edition > Langues du projet). Video : `pistes_audio: [{piste: n | fichier, langue, langue_id,
  decalage_ms}]` (`audio::AudioTrackCfg`, piste n = n-ieme piste audio du fichier, fichier = doublage separe). Sans pistes configurees :
  ancien fonctionnement (premiere piste audio, langue du disque, `langue`).
- `audio::plan_audio` : numero de piste audio d'une langue = rang parmi les langues ayant de l'audio dans au moins une video (ordre de la
  liste). Pas de piste de silence : chaque video doit avoir les PREMIERES langues de cet ordre (moins de langues permis, ni trou ni autre
  ordre) sinon erreur explicite ; une video sans reglage compte pour sa piste par defaut dans la langue du disque (doit etre dans la liste).
  Les pistes sont rangees dans cet ordre a la creation du disque.
- Disque : ffmpeg encode chaque piste en AC-3 stereo (`-itsoffset` pour un decalage, entree supplementaire pour un fichier audio) ;
  `Encoded.extra_audio`, PID audio 0x1100 + k, langue par flux dans la PMT, le CLPI (`make_clip_clpi_tracks`) et la playlist
  (`PlaylistParams.more_audio`, aussi dans « Tout lire ») ; compteurs de continuite TS par PID (`vmux::mux_film_audios`). Cle du cache
  d'encodage : reglages audio ajoutes seulement quand une liste de langues est utilisee.
- Interface : panneau « Pistes audio » par video (`audio_ui.rs`), « Langue audio par defaut » dans Disque. Reste a faire : bouton « Regler la
  langue audio » (commande SET_STREAM audio, format de l'operande a verifier). Tests : tests/audio.rs, project.rs (`several_audio_tracks_per_video`).

## Action « Regler la langue audio »
- Destination `Target::Audio(id_langue)` (JSON `{"audio_langue": id}`), distincte de `Target::Stay` (sous-titres) : un element regle l'un OU
  l'autre. Disponible pour tout element cliquable (bouton, vignette, texte, image) via « Destination » / « Au clic » : une entree par langue.
- Disque : commande `mobj::set_audio_stream(rang)` (SET_STREAM, dst = 0x8000_0000 | (rang+1) << 16, sous-titres intacts), rang = rang de la
  langue dans `AudioPlan.slots` ; sans piste correspondante : NOP. Format de l'operande d'apres libbluray, NON verifie sur lecteur.
- Retrait d'une langue : la destination redevient « a choisir ». Validation : avertissement si aucune video n'a d'audio dans cette langue.

## Reglages puis page de sortie (« Ensuite »)
- `Target::Stay(Option<page>)` et `Target::Audio(id, Option<page>)` : le reglage (sous-titres via `Item::sub`, ou audio) puis la page du
  menu principal choisie, ou rester sur la page (`None`). JSON : champ `"ensuite": n` (1-based) a cote de `"rester"` / `"audio_langue"`.
- Interface : destinations « Regler les sous-titres » et « Regler la langue audio : X » (sans « sans quitter la page ») ; sous la langue,
  liste « Ensuite » : « Rester sur la page courante » puis les pages. `Target::without_exit` sert a comparer avec les entrees de la liste.
- Renumerotation des pages (echange, deplacement, ajout, suppression, conversion) via `Target::page_mut` / `Target::page_removed` : un
  reglage dont la page de sortie est supprimee reste sur place.
- Disque : commandes [SET_STREAM, JUMP_TITLE page] (ou NOP sans sortie). Securite : un bouton sans commandes qui recoit un reglage de
  sous-titres garde son saut (JUMP_TITLE ajoute). Nouvel element cliquable : destination par defaut = premiere page (plus le reglage).

## Halo anime (etat selectionne anime)
- Style : `ButtonStyle::glow_pulse` (JSON `"halo_anime": true`), sans effet si le halo vaut 0. `PULSE_FRAMES` = 8 images ; image i =
  `pulse_style(i)` (rayon du halo entre 35 % et 100 %, en cosinus pour une boucle sans a-coup). Marges = celles du halo complet.
- Disque : `page_buttons` dessine les etats via une fonction de dessin rappelee pour chaque image (tous types d'elements, images
  d'etats comprises) ; `MenuButton::sel_frames`. `build_menu` ecrit l'etat selectionne en objets CONSECUTIFS [debut, fin] (exige par
  libbluray : objet = debut + index), drapeau repeat 0x80, et la cadence de page `anim_rate_code` (une image toutes les code+1 images
  video, environ 8 i/s). Poids : images supplementaires comptees (`state_count`, `weight`).
- Editeur : case « Halo anime (pulsation) » dans le style ; la simulation joue l'animation sur le bouton selectionne.
- Non verifie sur lecteur materiel. Tests : tests/anim.rs, project.rs (style du projet).

## Animation personnalisee de l'etat selectionne
- Images d'etats d'un element (`Skin`) : `sel_frames` (JSON `"animation_selection": [chemins]`) et `sel_once`
  (`"animation_une_fois": true`, sinon en boucle). Exige une image normale (`Skin::anim_frames` = 0 sinon). Prioritaire sur le halo anime.
- Disque : chaque image passe par `Renderer::skin_button` en tant qu'image « selectionne » (effets du style appliques) ; la premiere
  remplace l'etat selectionne, les autres vont dans `MenuButton::sel_frames` ; `sel_once` -> drapeau repeat a 0.
- Cadence par page : `PageDesign::anim_fps` (JSON `"animation_ips"`, 0 = `DEFAULT_ANIM_FPS` 8), une seule par page (norme) ;
  `MenuOptions::anim_fps` -> `anim_rate_code(code video, ips)`.
- Interface : inspecteur > images d'etats > « Animer l'etat selectionne » (liste ordonnee, boucle / une fois, « ▶ Apercu ») ; cadence
  dans « Fond de cette page ». Apercu et simulation jouent l'animation (en boucle). Validation : image normale manquante, fichiers absents.
- (suite) Une seule ligne « Selectionne » dans l'inspecteur : une image = etat fixe (`Skin::selected`), plusieurs (selection
  multiple) = animation (`Skin::sel_frames`, au moins deux). `Skin::set_selected` / `selected_files` gardent cet invariant ; a la
  lecture, une animation prime sur une image « selectionne » et une animation d'une image devient image fixe. Apercu statique de
  l'etat selectionne d'un element anime : premiere image (bdauthor-render).

## Etat normal anime
- `Skin::normal_frames` / `normal_once` (JSON `"animation_normal"`, `"animation_normal_une_fois"`), meme invariant que l'etat
  selectionne (`set_normal`, `normal_files`) ; `has_normal()` / `normal_file()` (premiere image) remplacent `normal.is_empty()` / `normal`.
- Disque : chaque image passe par `skin_button` en etat normal (assombrissement) ; `MenuButton::normal_frames`, objets consecutifs,
  drapeau repeat 0x80 sauf « une fois ». Poids : images ajoutees.
- Interface : ligne « Normal » comme « Selectionne » (une image ou plusieurs, liste par glisser-deposer, lecture, apercu).
  L'apercu (« ▶ Apercu » ou simulation) joue les animations de l'etat normal de tous les elements non selectionnes, meme horloge.

## Sons des menus
- `sound.rs` : `make_sound_bdmv` / `parse_sound_bdmv`, format d'apres libbluray (`sound_parse.c`) : « BCLK0200 », debut des donnees
  (u32), extension (u32 = 0), reserve jusqu'a l'octet 40, longueur de la table (u32), reserve (u8), nombre de sons (u8), puis par son :
  attributs (canaux 4 bits : 1 mono / 3 stereo ; frequence 4 bits : 1 = 48 kHz ; 2 bits : 1 = 16 bits ; 6 bits de bourrage), position
  relative au debut des donnees (u32), longueur en octets (u32) ; echantillons LPCM 16 bits gros-boutistes. Fichier BDMV/AUXDATA/sound.bdmv
  (sans copie BACKUP). 255 sons au plus.
- Style des boutons : `sound_select` / `sound_activate` (JSON `"son_selection"` / `"son_activation"`, ecrits seulement si non vides) ;
  un style propre d'element remplace celui du projet (sons compris). Les identifiants (ordre de rencontre) vont dans les champs son des
  etats selectionne / active des boutons IG (0xFF = aucun).
- Construction : `ffmpeg::decode_menu_sound` (stereo 48 kHz s16be, 10 s au plus). Validation : fichiers introuvables.
- Non verifie sur lecteur materiel.
- (suite) Sons propres a un element, independants de l'apparence : `Look::sounds: Option<ItemSounds>` (JSON element `"sons":
  {"selection", "activation"}`) ; `Look::sounds(&style_projet)` donne les sons effectifs (les siens, sinon ceux du style du projet).
  Les sons ne figurent plus que dans le style du projet ; ceux d'un style propre (format de quelques minutes) sont deplaces a la lecture.
  Inspecteur : case « Sons propres a cet element » au meme niveau que « Apparence propre a cet element ».

## Nettoyage du format de projet (ancien modele des menus automatiques retire)

- Le projet ne contient plus que des pages personnalisees : `mise_en_page` = `pages`, `style_boutons`, `accueil`.
  Les anciennes cles `principal`, `chapitres`, `libres`, `menu.par_page`, `menu.retour` ne sont plus lues ni ecrites.
- Cible de page en JSON : `{"page": n}` (l'ancien `"menu": "principal"` est ignore a la lecture).
- Retires : menus de chapitres separes (`Target::Chapters`), bouton « Retour » (`NavAction::Back`), `MenuType`,
  vignettes « element de la page », sous-titres par numero de piste, plan de navigation pagine (nav.rs).
- Sans aucune page, la construction genere des pages de depart (`convert::generate_pages`) ; l'editeur propose
  « Creer les pages de depart » (`editor::ensure_pages`).
- `projet_test.json` converti (copie : `projet_test.avant_nettoyage.json`).

## Image ISO UDF 2.50 (etape 1 : coeur)

- `iso.rs` (std seul) : `write_iso(dossier, sortie, &IsoOptions, annulation, avancement)`, `read_iso` (controle des
  etiquettes, CRC, positions, arborescence), `verify_iso` (structure + contenu octet par octet), `iso_size` (taille exacte
  sans ecrire), `fit_label` / `fit_info` (ce qui sera garde des noms), `CAPACITIES` (BD-25/50/100).
- Disposition : VRS 16-18, VDS principale 32, reserve 48, LVID 64, ancres 256 et dernier secteur, partition physique a 288.
  Partition de metadonnees (carte de type 2, unite 32 blocs) : FSD, entrees etendues (EFE) et repertoires ; donnees des
  fichiers en descripteurs longs dans la partition physique ; miroir distinct si l'option est active.
- Options : nom du volume (30 car. latins / 15 sinon), proprietaire/organisation/contact (IUVD), UDF 2.50/2.60,
  alignement des .m2ts sur 64 Ko, ordre « lecture » (navigation, videos, BACKUP, CERTIFICATE), miroir, date fixe
  (2000-01-01, identifiant de volume = empreinte du contenu : image reproductible).
- Ecriture dans `<sortie>.iso.part` puis renommage ; seuls BDMV et CERTIFICATE du dossier sont repris.
- Outil `bdiso <dossier> <image.iso> [nom]` (cree puis verifie) et `bdiso --lire <image.iso>`.
- Premier essai : `sortie_test.iso` (694 Mo) cree depuis `sortie_test`, a valider dans Windows et un lecteur.

## Image ISO (etapes 2 et 3 : construction + onglet « Génération ISO »)

- `IsoSettings` (cle « iso » du projet, seuls les reglages non par defaut sont ecrits) : `conserver` (dossier / iso /
  les_deux), `fichier`, `capacite` (BD-25/50/100), `nom_volume` (vide = titre), `proprietaire`, `organisation`, `contact`,
  `udf` (2.50/2.60), `aligner`, `ordre_lecture`, `miroir`, `date_fixe`, `verifier`.
- Construction (interface) : dossier seul = comme avant ; les deux = dossier choisi + `<dossier>.iso` (ou le fichier
  regle) ; image seule = construction dans `<image>.construction` (supprime apres l'image, ou apres un echec).
  `iso::finish_build` : avertissement de capacite, ecriture, verification, suppression du dossier temporaire.
- `bdauthor-build` ecrit aussi l'image si le projet la demande (le dossier est toujours garde en ligne de commande).
- Onglet « Génération ISO » (iso_ui.rs) : tous les reglages, compteur de caracteres du nom du volume.

## Palette des menus avec transparence

- Avant : 63 couleurs x 4 paliers d'alpha fixes (255/192/128/64) -> le halo (degrade d'opacite) devenait 2-3 bandes plates.
- `palette::quantize_rgba` : coupe mediane en (R, G, B, A) sur 253 entrees, poids de l'alpha x3, erreur de couleur
  ponderee par l'opacite ; seau coupe = effectif x etendue maximale. Images toutes opaques : `quantize_opaque` (tramage).
  Pas de tramage pour les images translucides (eviterait un scintillement entre images d'une animation).
- Mesure (test palette_alpha) : halo de 24 px -> 85 niveaux d'opacite et 0,9 d'erreur moyenne, contre 4 niveaux et 18.
  Pire cas (12 vignettes de bruit + 8 images de halo) : ~2 s en release.
- `quantize_images` reste pour les sous-titres (PGS).

## Image ISO : marge de securite, date choisie, compatibilite maximale

- `IsoSettings.margin` (cle « marge », defaut 95, 0 = desactivee) : `capacity_warning` previent au-dela de ce % du disque
  vise (en plus du depassement).
- `IsoOptions.date` : `IsoDate::Now` / `Fixed` (2000-01-01, cle « date_fixe ») / `Custom(IsoStamp)` (cle « date »,
  « AAAA-MM-JJ HH:MM », temps universel). Fixe ou choisie : identifiant du volume = empreinte du contenu (reproductible).
- `IsoOptions.compat` (cle « compatibilite », defaut vrai) : 288 secteurs en fin d'image, sequence de reserve a +16,
  ancre a N-257 et sur le dernier secteur ; sinon reserve au secteur 48 et ancre finale seule. `read_iso` controle la
  sequence de reserve et la troisieme ancre (`IsoTree.anchors`, `reserve_at`).

## Chapitres : liste unique par video, vignettes liees

- `ChapterEntry.id` (stable, propre a la video, jamais 0). JSON : `{"t", "titre", "id"}` ; l'identifiant n'est ecrit que pour
  les chapitres auxquels une vignette est liee (sinon un simple nombre ou `{t, titre}`). Les anciens fichiers recoivent des
  identifiants au chargement (`fix_chapter_ids`).
- `Kind::Slot.chapter: Option<u32>` (JSON `chapitre`, valable avec `depart_s`) : `start` reste l'instant utilise par la
  construction et l'apercu ; `Designs::sync_chapters` / `ProjectModel::sync_chapters` le realignent sur le chapitre
  (appele au chargement et a chaque image de l'interface). Chapitre disparu ou video changee : la vignette garde son
  instant et devient libre. Changer la video d'une vignette la delie (fait par l'interface).
- `VideoEntry::add_chapter` (reutilise un chapitre a 10 ms pres), `ProjectModel::chapter_uses`, `link_chapters_by_time`
  (pages de depart liees a leurs chapitres manuels). La construction du disque n'est pas modifiee.
- Interface : inspecteur de la vignette = liste « Chapitre de la vidéo » + « Créer le chapitre » + « Détacher » ;
  « Vidéo sélectionnée » affiche le nombre de vignettes liees a chaque chapitre.

## Reglages, chapitres, alignement
- Chemin de ffmpeg : Fichier > Configuration (`ffmpeg_path` dans config.txt). `Tools::set_custom_path` : dossier ou ffmpeg.exe, essaye avant BDAUTHOR_FFMPEG_DIR, le dossier du programme et le PATH.
- « Reutiliser les videos deja encodees (cache) » : deplace de la fenetre Projet vers Configuration (reste enregistre dans le projet).
- Chapitres / vignette d'une video : instants limites a la duree connue de la video.
- Editeur : Maj+clic = selection multiple (cadre bleu), glisser/fleches/Suppr s'appliquent au groupe ; panneau « Alignement » (gauche/centre/droite, haut/milieu/bas, centrer sur la page, repartir) : `editor::align_items`.

## Navigation manuelle aux fleches
- `Item.id` (identifiant stable dans la page, attribue seulement quand un lien le vise) et `Item.nav: NavLinks` (haut/bas/gauche/droite : `Auto` | `Stay` | `Item(id)`). JSON : `"id"`, `"navigation": {"haut": id | "aucun", ...}` (seules les fleches non automatiques sont ecrites).
- Disque : `MenuButton.nav_set` ([Option<u16>;4]) applique par-dessus `auto_neighbors` (menu.rs) ; `page_buttons` (project.rs) resout les identifiants en rangs de boutons. Lien perdu = automatique. Simulation : meme regle dans `editor::sim_buttons`.
- Editeur : rubrique « Navigation (fleches de la telecommande) » du widget selectionne ; fleches cyan sur l'apercu. Dupliquer / coller / changer de page efface id et liens de l'element copie ; supprimer un element efface les liens qui le visaient (`prune_nav`).

## Zoom de l'apercu, noms de widgets
- Apercu : molette = zoom (25 % a 800 %, centre sur le curseur), clic molette ou Espace + glisser = deplacer ; boutons − / + / Ajuster ; reinitialise au changement de page. Le rendu reste en 960x540 (flou a fort zoom, cadres nets).
- `Item.name` (JSON `nom`) ; copies nommees « Nom (copie N) » (`copy_name`), sauf couper/coller.

## Jaquette (version A : PDF image)
- Noyau : `cover.rs` (modele `Cover` / `CoverItem`, mm depuis le coin de la jaquette rognee, fond perdu en negatif, JSON cle `jaquette`, `layout(dpi)` -> `Decor` en pixels, `to_pdf`), `deflate.rs` (zlib LZ77 + Huffman fixe, sans dependance), `pdf.rs` (une page, image RVB Flate, traits de coupe / pliage). Formats : `FORMATS` (Blu-ray 14 / 11 mm, DVD 14 / 7 mm).
- Rendu : `FontRenderer::render_cover` (bdauthor-render) : zones colorees, image de fond, elements (rotation 90 / 270 : dessines a plat sur le morceau puis remis en place).
- Interface : onglet « Jaquette » (`cover_ui.rs`) : format, fond, liste d'elements, apercu avec reperes (rognage, pliages), glisser pour deplacer, export PDF. Pas d'annuler / refaire pour la jaquette.
- Valide : PDF relu par `pdfinfo` et `qpdf --check`. Pas encore : champs automatiques (titre, liste des videos), code-barres, etiquette de disque, PDF vectoriel.

### Jaquette : annuler/refaire, redimensionnement, champs, etiquette, export en arriere-plan
- Annuler/refaire (Ctrl+Z, Ctrl+Y ou Ctrl+Maj+Z, boutons ↶ ↷) : `cover::EditHistory<T>` generique, instantanes de (jaquette, etiquette),
  modifications rapprochees (< 600 ms, ex. un glissement) fusionnees en une etape, 200 etapes max. Remis a zero au chargement d'un projet.
- Redimensionnement a la souris : poignees aux 4 coins de l'element selectionne (rayon 9 px, curseurs adaptes), taille min 2 mm,
  valeurs arrondies a 0,1 mm ; le glisser part de l'element sous la souris (plus de deplacement de l'element precedent si on clique dans le vide).
- Champs automatiques dans les textes : {titre}, {liste_videos}, {nb_videos}, {duree_totale}, {nb_chapitres}, {annee}
  (`cover::FIELDS`, `apply_vars`, `ProjectModel::cover_vars`). Menu « Inserer un champ automatique ». La duree totale reste vide tant
  qu'une duree video est inconnue.
- Etiquette de disque (`cover::DiscLabel`, cle JSON « etiquette », ecrite seulement si modifiee) : diametre 118 mm, trou 25 mm, fond perdu 2 mm ;
  feuille carree ; le PDF decoupe la couronne (`pdf::image_pdf_shaped`, clip `W* n`) et peut tracer des cercles de repere.
  Editeur partage avec la jaquette via le trait `cover::SheetDoc` ; onglet Jaquette | Etiquette de disque.
- Export PDF dans un fil separe (message de progression, canal mpsc) ; `FontRenderer::render_sheet(&dyn SheetDoc, dpi, dir, vars)` remplace `render_cover`.

### Jaquette : calques et glisser-deposer
- `cover::CoverLayer {name, visible, locked}` ; `Cover.layers` / `DiscLabel.layers` (vide = un seul calque implicite « Calque 1 », rien d'ecrit dans le JSON) ; `CoverItem.layer`.
  JSON : « calques » (nom, visible, verrouille) et « calque » sur l'element s'il n'est pas 0.
- Ordre de dessin : `draw_order` (par calque puis ordre de la liste) ; un calque masque n'est ni affiche ni exporte ; un calque verrouille n'est plus selectionnable a la souris.
- Liste des elements groupee par calque (le plus en avant en haut) : glisser un element sur un autre le place devant lui (et dans son calque), sur l'en-tete d'un calque il passe au premier plan de ce calque.
  Boutons : + Calque, Supprimer le calque (elements reportes sur le voisin), Calque ▲▼, ▲▼ d'un element = dans son calque. Calque actif (●) : recoit les nouveaux elements.

### Jaquette : liste des calques au design de la gestion des pages
- Meme presentation que « Pages et calques » du mode Mise en page : cadre groupe, poignee ☰ (en gras pour un calque, grise pour un element), + / − pour replier, 👁/◌ masquer, 🔒/🔓 verrouiller,
  ✏ ou double-clic pour renommer, 🗑 (supprimer le calque, ses elements au voisin ou supprimes), « (n) » = nombre d'elements, + tout / − tout.
- Depot : trait bleu a l'endroit d'insertion calcule d'apres la position du pointeur (sous un en-tete : premier plan du calque ; sous un element : juste derriere lui ; un calque s'insere entre deux calques).

### Jaquette : zoom et regles
- Zoom 25 % a 1600 % de la vue (molette centree sur le curseur, boutons − + Ajuster, 1:1 = taille reelle approximative a 96 dpi, pourcentage affiche relatif a la taille reelle) ; deplacement clic molette ou Espace + glisser.
  Etat propre a l'onglet (cover_zoom, cover_pan), remis a zero au changement Jaquette | Etiquette.
- Apercu plus fin quand on zoome : 96, 144 ou 192 dpi selon l'echelle a l'ecran (`cover_tex_dpi`), re-rendu seulement au changement de palier.
- Regles graduees en haut et a gauche, mm ou pouces (bascule), zero au coin haut gauche de la feuille rognee, pas adapte au zoom, trait du pointeur, bande de l'element selectionne ; coordonnees du pointeur dans la ligne d'aide.

### Jaquette : export PNG et impression A4
- `png::write_png_print(img, dpi)` : PNG compresse (zlib maison), RVB si opaque, resolution enregistree (pHYs). Case « PNG rogne au format fini » (retire le fond perdu).
- `SheetDoc::to_pdf_a4` : page A4 portrait ou paysage (portrait si possible, avec 8 mm de marge pour les reperes), jaquette ou etiquette a sa taille reelle, centree.
  Jaquette : traits de coupe aux coins et de pliage de la tranche ; etiquette : disque decoupe en couronne, contour et trou traces, 4 reperes en croix. Erreur claire si le format fini ne tient pas sur A4.
  A imprimer a 100 % (sans « ajuster a la page »). Boutons « Exporter en PDF », « Exporter en PNG », « Imprimer sur A4 » : tous en arriere-plan.

### Jaquette : choix de la police par menu deroulant
- Meme menu que le widget texte de la mise en page (`bdauthor_render::common_fonts()` : polices usuelles installees, « Police par defaut », « Autre fichier… » .ttf/.otf stocke relatif au projet) ; message rouge si la police est introuvable.

### Redimensionnement proportionnel (Maj)
- `editor::dragged_rect_ratio` : en tirant un coin avec Maj, le coin oppose reste fixe et les proportions d'origine sont conservees (echelle = la plus grande des deux dimensions, bornee par la taille minimale et par le plan 1920x1080).
  Valable pour tous les widgets de la mise en page (hors selection multiple, qui n'a pas de poignees) et pour les elements de la jaquette / etiquette (meme principe, en mm, sans borne de plan).

## Jaquette : accrochage, sélection multiple, formes, texte enrichi (export 116)
- Aimantation (case « Aimanter », Alt = désactivée) sur bords/centres de la feuille, pliages, autres éléments ; traits magenta ; zone de sécurité 3 mm (pointillés orange) ; flèches : 0,5 mm (Maj 5 mm, Ctrl 0,1 mm).
- Sélection multiple (Maj/Ctrl+clic, rectangle de sélection, liste), déplacement de groupe, Ctrl+C/X/V (jaquette ↔ étiquette), Dupliquer/Supprimer, panneau d'alignement (`align_cover`).
- Formes : rectangle, ellipse, ligne (remplissage, bordure, coins arrondis). Texte : contour, ombre, interligne, alignement vertical, ajustement au cadre.
- Core testé (122 tests) ; render et gui à compiler avec recompiler.bat.

## Jaquette : selection multiple en tirets (export 117)
- Les elements en selection multiple ont le meme rendu que l editeur de menu (fond sombre + tirets blancs).

## Jaquette : ligne manipulee par ses extremites (export 118)
- CoverKind::Shape gagne `flip` (JSON "inverse") ; `CoverItem::line_ends/set_line_ends` ; cadre de largeur ou hauteur nulle autorise pour une ligne.
- GUI : deux poignees (Maj : pas de 45 degres, aimantation, autre extremite comme repere), selection au clic sur le segment, champs Debut/Fin X,Y. Rendu : segment a bouts arrondis (`draw_line`).
- Une ligne creee avant cet export (cadre h=2 mm) devient legerement oblique.

## Jaquette : types de trait et fleches (export 120)
- LineStyle (plein, tirets, pointilles, tirets+points, double) et LineArrow (aucune, debut, fin, deux) sur CoverKind::Shape ; JSON "trait" / "fleche" (ecrits seulement si non par defaut).
- `move_line_end` : les fleches suivent leur extremite quand les bouts se croisent. Rendu : `draw_segment` (motifs le long du trait, pointes en triangle lisse).

## Sauvegarde automatique du projet (export 121)
- Option Configuration : sauvegarde auto toutes les N min (defaut 2), uniquement si le projet a change et sans bouton souris enfonce. Fichier `<projet>.autosave.json` a cote du projet (projet sans nom : `recuperation.autosave.json` dans le dossier de config). Le projet lui-meme n est modifie que par Enregistrer.
- Ecriture atomique (fichier .tmp puis renommage), aussi pour Enregistrer. Enregistrer supprime la sauvegarde auto.
- A l ouverture (et au demarrage), si la sauvegarde auto est plus recente que le projet : fenetre Restaurer / Ignorer / Supprimer.

## Liste des videos, reperes de chapitre, apercu des sous-titres (export 122)
- Liste des videos : bouton supprimer dessine (croix) et nom raccourci pour que les boutons restent visibles.
- Option « Automatiques, un toutes les » retiree de l interface (le champ auto_s reste lu pour les anciens projets, avec un bouton « Les retirer »).
- Sous-titres : « Aperçu du rendu… » par piste (fichier ou piste integree) : fenetre avec navigation dans les repliques, fonds, 1080p/720p, zone de securite, edition du style en direct.

## Apercu des sous-titres : fond « Image de la video » (export 123)
- Quatrieme fond : image reelle de la video a l apparition de la replique (extract_thumbnail_rgba en arriere-plan, cache de 6 images).

## Fenetres systeme (export 124)
- `os_window` (viewport immediat eframe) pour Configuration, Projet et Apercu des sous-titres : fenetres deplacables hors du cadre principal. Repli sur une fenetre flottante si le backend ne gere pas les viewports.

## Apercu des sous-titres : zoom (export 126)
- Boutons - + Ajuster 1:1, molette (ou Ctrl+molette) sur l image, glisser pour deplacer ; pourcentage = taille reelle de la video.

## Formats de sous-titres (export 127)
- Lecture directe : SRT, WebVTT (horaires mm:ss.mmm acceptes), MicroDVD (cadence par {1}{1}fps, sinon 23,976), SubViewer 2 et SBV, SAMI (premiere classe de langue). Format reconnu d apres le contenu (sauf .ass/.ssa).
- Autres extensions (TTML, LRC, STL...) : conversion par ffmpeg (`convert_subtitle_file_to_srt`) si le lecteur direct ne trouve rien.
- Tests : tests/subtitle_formats.rs.

## Cadence par piste de sous-titres (export 128)
- SubTrackCfg.fps (JSON "cadence_ips", ecrit seulement si defini) pour les fichiers a temps en images (MicroDVD) ; `load_cues_fps` / `parse_file_text_fps` ; liste de cadences dans le panneau (visible pour .sub et .txt), prise en compte a la construction et dans l apercu.

## Correctif fenetre Configuration + cadence automatique (export 129)
- La fenetre Configuration (fenetre systeme) n appelait os_window qu a chaque image meme fermee : elle s affichait au demarrage et ne se fermait pas. Maintenant appelee seulement si ouverte.
- Cadence MicroDVD par defaut = cadence sondee de la video (si courante : `plausible_fps`), sinon 23,976 ; reglage manuel de la piste prioritaire.

## Apercu des sous-titres : zone du bas ajustable (export 130)
- `os_window_panels` : TopBottomPanel redimensionnable en bas (style de la piste), contenu principal sans defilement propre ; l image occupe la hauteur restante.

## Textes d explication -> boutons « ? » (export 132)
- `help(ui, texte)` (main.rs) : petit bouton « ? » dont le texte s affiche au survol ; 40 paragraphes explicatifs (.weak()) convertis automatiquement (textes litteraux de 40 caracteres ou plus ; les libelles d etat et textes formates restent).

## Options « Menu » deplacees dans Projet (export 134)
- Onglet Projet > « Style des pages » : image et duree du fond, video de fond (+ son), musique, boucle. Onglet Général : sequence d ouverture et menu 1920x1080. L onglet Videos ne garde que les reglages de la video selectionnee.

## Export 136 — réglages avancés des sous-titres
- Style de piste : alignement gauche/centre/droite + marge latérale, largeur maximale (%), coupe des lignes longues (équilibrée), ombre portée (px, couleur, opacité), boîte derrière le texte (opacité, couleur, marge, coins). JSON : clés écrites seulement si non défaut.
- Piste : option « Retirer les indications [..] et (..) » (`retirer_indications`).
- `pgs::place` (position commune disque/aperçu), paliers d'opacité PGS ajoutés pour boîte et ombre.
- Aperçu : panneau « Contrôle de qualité » (vitesse, longueur/nombre de lignes, durées, intervalle) avec saut à la réplique.
- Non fait : positions par réplique ({\an8}), car cela demande un champ de plus dans `Cue`.
- Core : 136 tests verts. Render/GUI à compiler avec recompiler.bat.

## Export 137 — positions des répliques données par le fichier
- `Cue.pos: Option<CuePos>` (alignement + position verticale) lu depuis les balises ASS `\an1`..`\an9` (fichiers ASS et SRT).
- `pgs::render_cue_at` / `with_pos` : la position de la réplique remplace celle de la piste (bloc et alignement du texte).
- `merge` : répliques simultanées dans un seul bloc, à la position de la première qui en demande une ; pas de soudure si les positions diffèrent.
- Piste : case « Respecter les positions du fichier » (`positions_fichier`, vrai par défaut, écrit seulement si faux).
- Aperçu : la position demandée par la réplique est affichée et appliquée.
- Fichiers d'essai : code/exemples_soustitres/test_placements.ass et .srt (13 répliques, 9 positions).
- Core : 141 tests verts. GUI à compiler avec recompiler.bat.

## Export 138 — alignement des styles ASS / SSA
- `parse_ass` lit les sections `[V4+ Styles]` / `[V4 Styles]` : l'alignement du style de chaque réplique donne sa position (pavé numérique en ASS ; 1-3 bas, 5-7 haut, 9-11 milieu en SSA).
- La balise `\an` de la réplique l'emporte sur son style. Bas centre (défaut) ne demande rien : la position de la piste s'applique.
- Noms de styles : casse et astérisque initial ignorés. Marges de style non lues.
- Essais : exemples_soustitres/test_styles.ass, test_styles_ssa.ssa (test_webvtt.vtt : réglages WebVTT non gérés).
- Core : 143 tests verts. Aucun changement GUI/render.

## Export 139 — encodages, entités HTML, mise en forme, positions WebVTT
- `charset.rs` (+ `charset_tables.rs`, `charset_data/*.bin`, générés avec les codecs Python) : Windows-1250/1251/1252/1253/1254/1255/1256/1257/1258/874, Shift-JIS, GBK, Big5, CP949, sans dépendance. `Charset::Auto` = UTF-8 si valide, sinon page de code de la langue de la piste (`for_language`). Réglage de piste « encodage » (JSON `encodage`), `load_cues_enc`. L'aperçu relit le fichier quand l'encodage ou la langue changent.
- `entities.rs` (HTML 4.01 + apos) et `decode_entities` : nommées, `&#233;`, `&#xE9;`. Un `<` issu d'une entité n'est plus pris pour une balise.
- Mise en forme : marqueurs U+0001 dans `Cue::text` (`parse_runs`, `plain_text`, `convert_markup`, `strip_format`). Balises HTML (i, b, u, em, strong, font color, c.couleur), blocs ASS (`\i \b \u \c \1c \r`, transformations ignorées), MicroDVD (`{y:i}`, `{c:$..}`). Réglage de piste `mise_en_forme` (vrai par défaut).
- Rendu : italique (cisaillement), gras (double passe), souligné et couleur par morceau ; coupure des lignes sur les morceaux.
- WebVTT : `line:`, `position:`, `align:` -> `CuePos` (champs optionnels : le sens non demandé garde celui de la piste). `size:` non géré.
- Core : 151 tests verts. Render/GUI à compiler avec recompiler.bat.

## Export 140 — confirmation avant de retirer une vidéo
- La croix de la liste des vidéos ouvre une fenêtre « Retirer la vidéo ? » (nom, nombre de chapitres, pistes de sous-titres et audio perdus, rappel que le fichier n'est pas supprimé du disque) avec Retirer / Annuler ; Échap annule.
- GUI uniquement (`main.rs`, champ `confirm_remove`), à compiler avec recompiler.bat.

## Export 141 — traduction de l'interface (i18n), phase 1 à 2
- Les textes du code sont en **anglais** et servent de clés : `tr("Text")` (type `&'static str`, utilisable partout où un littéral l'est) et `trf!("{} min ago", n)` pour les textes avec valeurs (`{}` dans l'ordre, `{0}`, `{1}`… pour changer l'ordre dans une langue).
- `bdauthor-gui/src/i18n.rs` : table anglais -> langue, `set_language`, `available`, `template`, `num` (virgule décimale en français). Texte sans traduction = texte anglais affiché tel quel.
- `bdauthor-gui/lang/fr.json` (intégré au programme par `include_str!`) : traduction française, générée à partir des anciens textes français (661 clés).
- Autres langues sans recompiler : un fichier `<code>.json` dans le dossier `langues` à côté du programme (clé `"_name"` = nom affiché). Configuration > « Créer une traduction… » écrit le modèle (toutes les clés, valeurs vides).
- Langue enregistrée dans `config.txt` (`lang=fr` par défaut) ; liste « Langue / Language » dans Configuration, effet immédiat.
- Convertis : main, editor_ui, cover_ui, subs_ui, style_ui, iso_ui, audio_ui, sim_ui. Noms de copies : « (copie) » ou « (copy) » reconnus. Noms de zones de la jaquette (fournis par le cœur en français) traduits par `zone_label`.
- **Reste en français** (cœur) : messages de `check`, journal de construction, noms de langues (`default_language_name`), formats de jaquette, étiquettes d'actions de navigation, libellés d'encodage. Phase 3 éventuelle.
- Limites : écritures de droite à gauche non gérées par egui ; polices à ajouter pour les écritures non latines.
- GUI uniquement, à compiler avec recompiler.bat (non compilé dans le conteneur ; `i18n.rs` seul compilé et testé).

## Export 142 — traduction i18n, phase 3 (cœur)
- Le module `bdauthor_core::i18n` (`tr`, `format_tr`, macro `trf!`, `num`, `set_table`, `set_table_from_json`) porte maintenant la table de traduction ; l'i18n du GUI la remplit (même `lang/fr.json`, une seule table pour le cœur et l'interface).
- Textes du cœur passés en anglais (clés) : vérificateur `check` (messages, ERROR/warning/ok), validation du modèle et du projet, journal de construction, erreurs ffmpeg, audio, sous-titres (contrôle qualité, JSON), ISO (avertissements, messages, `gb()` avec `num`), éditeur, design (problèmes de page, libellés d'actions), jaquette (traits, flèches, formes, formats, champs automatiques), encodages, noms de langues (`default_language_name`).
- Constantes `FORMATS` et `FIELDS` de la jaquette : textes anglais, traduits à l'affichage par le GUI (`tr(f.0)`, `tr(desc)`).
- Restent en français volontairement : textes dessinés sur le disque (« Tout lire », « Retour », « Chapitres : … », « Vidéos »), valeurs par défaut stockées dans le projet (« Mes vidéos », « Menu », « Chapitre N », « Page N », « Calque 1 »), noms de zones de la jaquette (traduits par `zone_label` du GUI), messages techniques de bas niveau (ISO/UDF détaillés, flux, pgs, film, sound), CLI `bdauthor-build`.
- Sans table chargée (CLI, tests), le cœur écrit en anglais ; les essais chargent `fr.json` avec `tests/common/mod.rs` (`common::fr()`).
- Core : 151 tests verts. GUI à compiler avec recompiler.bat.

## Export 144 — largeur des menus après changement de langue
- egui garde la taille d'un menu ou d'une liste déroulante depuis sa première ouverture : après un changement de langue, `memory.reset_areas()` l'oublie pour qu'elle s'adapte aux nouveaux textes (les fenêtres internes reprennent aussi leur place par défaut).
- GUI uniquement (`main.rs`), à compiler avec recompiler.bat.

## Export 145 — éditeur de chapitres (fenêtre avec image, lecture et frise)
- Bouton « Éditeur de chapitres… » sous la liste des chapitres d'une vidéo (la liste reste pour les retouches rapides). Fenêtre `chapters_ui.rs` : image exacte à l'instant choisi (960 px de large), image par image / ±1 s / ±10 s / début / fin, lecture sans son, raccourcis (← →, Maj, Ctrl, Espace, C, Début, Fin), liste des chapitres (nom, aller, supprimer, placer le chapitre sélectionné ici), saut à l'image clé précédente/suivante de la source.
- Frise : graduations adaptées au zoom (molette autour du pointeur, Maj + molette ou barre pour défiler), vignettes de toute la vidéo (48 images clés, en arrière-plan), images clés de la source quand la vue fait moins de 10 min, repères de chapitre à glisser, tête de lecture ; clic ou glisser = aller à cet instant.
- Vignettes : « Utiliser l'image affichée » pour la vignette de la vidéo (`vignette_s`) et pour les vignettes de page liées au chapitre sélectionné (`thumb_at` de chaque vignette liée) ; retour à automatique / « suit le chapitre ».
- Brouillon jusqu'à « Valider » (« Annuler » ou fermer : rien ne change). Instants enregistrés au dix-millième de seconde (image exacte).
- Option par vidéo « Image clé exacte sur chaque chapitre » (JSON `chapitres_exacts`, écrit seulement si vrai) : l'encodage reçoit `-force_key_frames` (instants avancés de 0,5 ms) + `-forced-idr 1`, la clé de cache inclut ces instants ; calage des repères avec une tolérance de 45 tics (0,5 ms).
- Cœur : `ffmpeg::frame_args`, `extract_frame_rgba` (image exacte ou image clé, sur la sortie standard), `FrameStream` (lecture temps réel, arrêtée à la destruction), `list_keyframes` / `parse_keyframes_compact`, `force_keys_args`, `EncodeOpts::force_keys`. Essais : tests/chapter_editor.rs (dont un essai réel ffmpeg : image clé forcée à 1,48 s retrouvée dans l'encodage).
- Core : 156 tests verts. GUI à compiler avec recompiler.bat (non compilé dans le conteneur).

## Export 147 — menu Fichier, À propos, habillage du widget Texte
- Fichier : « Nouveau projet », « Ouvrir projet… », « Quitter ». Fermeture (Quitter ou croix de la fenêtre) : confirmation si le projet n'est pas enregistré ou si une construction tourne (Enregistrer et quitter / Quitter sans enregistrer / Annuler, Échap = annuler).
- Menu « À propos » > « À propos de bdauthor… » : version, description, bibliothèques et licences, emplacement des réglages et du dossier des langues.
- Widget Texte : fond (couleur, opacité 0-100 %, coins arrondis) sur tout le cadre et contour des lettres (épaisseur, couleur), tailles pour un menu 1920 × 1080, mises à l'échelle du menu. JSON de l'élément : `fond_couleur`, `fond_opacite`, `fond_arrondi`, `contour`, `contour_couleur` (écrits seulement s'ils servent). Choisir une couleur de fond (ou de contour) alors que l'opacité (ou l'épaisseur) vaut 0 la met à 60 % (ou 3 px).
- Cœur : `design::TextDeco` (champ `deco` de `Kind::Text` et de `project::Decor::Text`). Rendu : `bdauthor_render::draw_deco_text` (fond `rounded_rect` + contour par décalages en cercles), utilisé pour les textes du fond, les textes cliquables (boutons) et l'aperçu de l'éditeur.
- Essais : tests/text_deco.rs. Core : 160 tests verts. Render/GUI à compiler avec recompiler.bat.

## Export 148 — éditeur de menu : alignement, poignées de côté, Image de la vidéo
- Les problèmes de la page (chevauchements, destinations…) s'affichent au-dessus de l'aperçu.
- Aimantation « Aligner sur les autres éléments » (case à côté de « Grille 10 px », Alt : désactivée le temps du geste) : en déplacement, bords et centre ; en redimensionnement, les bords tirés ; sur les bords et le centre de l'écran et des autres éléments visibles ; traits magenta. Cœur : `editor::snap_lines`, `snap_item_rect`.
- Poignées de côté (gauche, droite, haut, bas) sur l'élément sélectionné : redimensionnement dans un seul sens (`Handle::Left/Right/Top/Bottom`, Maj sans effet sur un côté).
- Nouvelle page : plus de bouton « Suivant » ajouté automatiquement à la page précédente (la nouvelle page garde son « Précédent »).
- « + Calque » déplie la page dans l'arbre. Liste « Calque » retirée de l'inspecteur (le glisser-déposer de l'arbre suffit).
- Widget « Image de la vidéo » (palette, icône appareil photo) : `Kind::Image { frame: Some(VideoFrame { video, at }) }`, JSON `{"type":"image","video":n,"instant":s}` ; inspecteur : vidéo + instant ; cliquable comme une image. Aperçu : miniature 480×270 du fil de travail, écrite en PNG temporaire ; construction : image 1920×1080 extraite par ffmpeg dans le dossier temporaire. Vidéos retirées / échangées : renumérotation ; vérification « choisissez la vidéo » / « vidéo inexistante ». Image vide ou pas encore extraite : cadre sombre dans l'aperçu.
- Essais : tests/editor.rs (côtés, aimantation), tests/video_frame.rs, tests/project.rs (construction avec une image de vidéo et un texte habillé). Core : 164 tests verts. Render/GUI à compiler avec recompiler.bat.

## Export 149 — sélecteur d'image et recadrage du widget « Image de la vidéo »
- Inspecteur : bouton « Choisir l'image… » (fenêtre `frame_ui.rs`) : image exacte à l'instant choisi, pas image par image (boutons, ← →, Maj 1 s, Ctrl 10 s), barre de temps avec 40 vignettes de la vidéo, tête de lecture, aperçu agrandi au survol, clic ou glisser pour choisir l'instant. Valider / Annuler.
- Recadrage : case « Recadrer l'image », cadre à glisser (déplacer, angles, côtés, ou nouveau cadre tracé en dehors), proportions libres / celles du widget / 16:9 / 4:3 / 1:1, « Plus grand cadre ». Sans recadrage : centre de l'image en 16/9 (comme avant).
- Modèle : `VideoFrame::crop` (fractions de l'image entière), JSON `recadrage: [x, y, l, h]` ; `crop_px` ; cœur `editor::crop_drag`, `crop_full`.
- Construction : image entière (1920 de large, proportions de la vidéo) extraite à l'instant exact, puis zone gardée ; aperçu : image entière 640 de large (`Job::Frame`), recadrée, PNG temporaire.
- Essais : tests/video_frame.rs (recadrage JSON/pixels, poignées), tests/project.rs (image recadrée). Core : 166 tests verts. GUI à compiler avec recompiler.bat.

## Export 151 — poignées d'un widget plein écran, images de vidéo mutualisées
- Éditeur de menu : les poignées d'un élément qui touche un bord du plan sont rentrées d'au moins 6 px écran (`editor::handle_edges`, même calcul pour le dessin et la détection) ; un widget plein écran reste redimensionnable.
- Nouveau module cœur `framesrc.rs`, sans dépendance graphique : `probe_async` (durée, cadence, taille d'affichage), `FrameLoader` (extraction d'images en arrière-plan, une seule à la fois : la plus récente ; mémoire bornée ; échecs retenus ; `Fit::Stretch` image exacte / `Fit::Cover` fond 16/9 rogné), `Strip` (vignettes de la frise). Utilisé par l'éditeur de chapitres (`chapters_ui.rs`), le sélecteur d'image (`frame_ui.rs`) et l'aperçu des sous-titres (`subs_ui.rs`, fond « Image de la vidéo ») : les trois copies du même code de fil de calcul sont supprimées.
- Essais : tests/framesrc.rs (taille, sondage, mémoire et oubli, échec non redemandé, fond rogné, frise), tests/editor.rs (poignées plein écran). GUI à compiler avec recompiler.bat.

## Export 152 — barre de temps commune
- Nouveau `timeline_ui.rs` : `Timeline` (graduations, vignettes calculées en arrière-plan via `framesrc::Strip`, zoom molette, défilement Maj + molette, curseur de défilement, tête de lecture, aperçu agrandi au survol) ; l'appelant dessine ses repères par une fonction `overlay` et gère la souris à partir de `Geo` (`x_of`, `t_at`). Utilisée par l'éditeur de chapitres (repères de chapitre, images clés) et le sélecteur d'image (qui gagne graduations, zoom et défilement).
- Texte ajouté à fr.json : aide de la barre du sélecteur d'image. GUI à compiler avec recompiler.bat.

## Export 153 — lecture dans le sélecteur d'image
- `frame_ui.rs` : bouton ▶ Lecture / ■ Pause et touche Espace (flux `FrameStream`, sans le son) ; toute navigation (boutons, flèches, barre de temps) arrête la lecture ; la barre de temps suit la tête de lecture. « Valider » garde l'image affichée à ce moment.

## Export 154 — lecture et frise dans l'aperçu des sous-titres
- `subs_ui.rs` : frise commune (`timeline_ui`) sous les réglages de fond, avec les répliques de la piste en repères (la réplique choisie en orange) ; clic ou glisser = instant libre, fond « Image de la vidéo » activé d'office. Bouton ▶ Lecture / ■ Pause et Espace : la vidéo défile (sans le son, 720 lignes au plus) avec la réplique en cours dessinée par-dessus, en 16/9 rogné comme le fond fixe.
- Deux modes : instant lié à la réplique choisie (curseur, < >, « La plus longue », liste des problèmes : comme avant) ou instant libre (frise, lecture : la réplique affichée est celle qui est active, rien entre deux répliques ; le curseur de réplique suit).
- Cœur : `FrameStream::start_fit` (option `cover`) ; test `stream_cover_fills_requested_size`. Rendu refactoré en `sub_sprite` / `blank` / `blend` / `render_preview` (réplique dessinée une fois par réplique pendant la lecture).

## Export 155 — frise de l'aperçu des sous-titres sous l'image
- `subs_ui.rs` : le bouton Lecture, l'instant et la frise passent sous l'image (la zone de l'image réserve leur hauteur) ; la logique de lecture reste avant l'image.

## Export 156 — vignettes de la frise recalculées à l'échelle du zoom
- Cœur : `Strip::start_range(fichier, de, à, n, taille, exact)` (vignettes d'une plage ; `start` en est le cas particulier « toute la vidéo, images clés »). Test `strip_range_covers_only_the_range`.
- `timeline_ui.rs` : quand une case de la frise représente nettement moins de temps qu'une des 48 vignettes de la vidéo entière, la vue visible (plus une demi-vue de chaque côté) reçoit ses propres vignettes, une par case, calculées en arrière-plan une fois la vue restée stable 250 ms ; images exactes si une case représente moins de 10 s (sinon image clé la plus proche). Les vignettes de la vidéo entière servent en attendant, et pour les cases hors plage. Un nouveau calcul n'est lancé que si la vue sort de la plage calculée ou si l'échelle change de plus de 20 %. S'applique aux trois fenêtres (chapitres, sélecteur d'image, aperçu des sous-titres).

## Export 157 — éditeur de menu, fenêtres d'aperçu

- Chevauchement de widgets : plus de texte d'avertissement, contours rouges clignotants dans l'aperçu.
- Nouveaux widgets Rectangle et Ligne (code de l'éditeur de jaquette : `Kind::Shape`, `Paint`, `Decor::Shape`, rendu dans `bdauthor-render`). Ligne : taille 0 possible (horizontale/verticale), style, flèches, retournement.
- Collage dans l'éditeur de menu : placé dans la zone vide la plus proche (`editor::free_spot`).
- Liste calques/widgets : défilement automatique pendant le glisser-déposer, icônes agrandies (17 pt), icône « calque caché » corrigée (œil barré).
- Menu Édition > « Générer le disque » : fenêtre dédiée (bouton, progression, résultat, journal) à la place du panneau du bas.
- Sous-titres et pistes audio : langue, ✏ et 🗑 à droite de chaque piste ; bouton 🔊 sur les pistes audio = fenêtre avec forme d'onde + lecture (ffplay, `ffmpeg::AudioPlayback`, `audio_peaks`).
- Vignette alternative d'une vidéo : bouton « Choisir l'image… » (même fenêtre que le sélecteur d'image : aperçu + frise).
- Le GUI et le rendu ne sont pas compilés dans le cloud : compiler avec recompiler.bat. ffplay.exe doit être à côté de ffmpeg.exe pour l'écoute audio.

## Export 158 — langues audio propres à chaque vidéo

- Plus de contrainte « mêmes langues audio dans le même ordre pour toutes les vidéos » : chaque vidéo a ses propres langues (pistes rangées dans l'ordre de la liste des langues).
- Bouton « langue audio » : retient l'identifiant de langue dans le registre r2 (`R_AUDIO`) au lieu de fixer un numéro de piste.
- Début de chaque vidéo (titre simple, lecture depuis un repère, « Tout lire » d'après la 1re vidéo) : `CMP_EQ r2, id` + `SET_STREAM` pour chaque piste ; langue absente : le lecteur garde son choix. Une seule piste : aucune commande.
- Journal de construction : remarque pour chaque vidéo à qui il manque une langue audio du projet.
- Non vérifié sur lecteur matériel (même réserve que le réglage audio précédent).
