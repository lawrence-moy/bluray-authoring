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

## Plan (superposition) et contenu des vignettes
- L'ordre de la liste d'elements d'une page = ordre de superposition (le dernier est au premier plan), tous types confondus.
  Fond : textes/images dans l'ordre. Boutons (IG) toujours au-dessus du fond ; un texte/image place devant un bouton est
  pose sur les images du bouton (`Renderer::overlay`, seulement sur l'intersection avec le cadre du decor).
- Vignette : champ `assign` (JSON `element`, 1-based) = element de la page montre ; sinon automatique. `slot_assignment`
  (valide, sans doublon, le reste dans l'ordre) et `assign_slot` (echange avec la vignette qui avait deja cet element).
- Editeur : boutons Tout derriere / Derriere / Devant / Tout devant, menu « Contenu » d'une vignette, liste des elements.
- `copy_decor` conserve l'ordre de superposition de la page source.

## Audio sans perte (LPCM) par piste
- Format choisi par piste (`AudioFormat` : AC-3 par defaut, LPCM 16 bits, LPCM 24 bits), cle JSON `format` ecrite seulement hors AC-3.
  Encodage ffmpeg `pcm_bluray`, flux de type 0x80 dans le TS, le CLPI et la playlist (module `lpcm`). 48 kHz uniquement.
- Debit LPCM = canaux (arrondis au pair) x bits x 48 kb/s ; le debit de pointe de la video est plafonne a `MUX_BUDGET_KBPS` (33 700)
  moins l'audio, erreur si moins de 8 000 kb/s restent.
- **Dispositions de canaux** : l'encodeur `pcm_bluray` de ffmpeg n'accepte que mono, stereo, 3.0, 3.0(back), 4.0, quad(side),
  5.0(side), 5.1(side), 7.0 et 7.1. Les sources 5.1 passent telles quelles (6 canaux). Les 5.0 (surround arriere) et 6.1 sont remixees
  par ffmpeg en 7.0 / 7.1 (8 canaux, fichier ~37 % plus gros, constate sur des sons de test) ; l'estimation de debit compte donc
  8 canaux pour elles (prudent, pas exact). Pistes possibles : suivre le remixage reel, ou imposer la disposition (5.0 -> 5.1 + LFE vide).
- **A verifier sur materiel** (non confirme, aucun lecteur ici) : octets du descripteur d'enregistrement HDMV, et trames de 7,5 ms
  (360 echantillons) en 24 bits. Test prevu : BD-RE avec une piste LPCM 16 bits et une 24 bits.
- Non fait : TrueHD / DTS-HD (sources deja encodees, copie directe), 96 / 192 kHz.

### Audio AC-3 par piste : 5.1, debit manuel, copie
- Formats de piste : `ac3` (2.0, defaut), `ac3_51` (5.1, 448 kb/s par defaut ; au-dela de 6 canaux, mixage en 5.1 ; une source stereo
  ne remplit que l'avant), `ac3_copy` (flux AC-3 de la source recopie), `lpcm16`, `lpcm24`.
- Debit manuel (`bitrate_kbps`, debits AC-3 32 a 640) pour `ac3` et `ac3_51` ; absent = debit du projet (2.0) ou 448 (5.1).
  Le LPCM n'a pas de reglage de debit (canaux x profondeur x 48) ; la copie garde celui de la source.
- Copie : refusee avant l'encodage si la source n'est pas de l'AC-3, pas a 48 kHz, a plus de 6 canaux ou a plus de 640 kb/s
  (message explicite). Le decalage de piste (`-itsoffset`) fonctionne aussi avec la copie.
- Le cache d'encodage tient compte du format et du debit des pistes (les pistes AC-3 par defaut gardent leur ancienne cle).

## Son dans les apercus video
- Lecteur commun `bdauthor-gui/src/player.rs` (fenetres : choix de l'image d'une video, chapitres, apercu des sous-titres).
- Son decode par ffmpeg (`playback::PcmFeed`, flottants entrelaces au format de la sortie : frequence et canaux de la carte son,
  ffmpeg reechantillonne / remixe) et joue par cpal (WASAPI / CoreAudio / ALSA). Tampon d'avance 0,4 s.
- Le son donne le temps : echantillons joues moins le delai de la sortie (horodatage cpal). Les images (`FrameStream::start_paced`,
  sans `-re`) sont montrees quand leur instant est atteint. Sans son (muet, piste absente, sortie absente ou muette 1,5 s, fin de la
  piste), `PlayClock` continue sur le temps ecoule sans saut. Si le decodage video prend plus de 0,5 s de retard, le flux d'images
  reprend a l'instant du son (au plus toutes les 2 s).
- Choix du son (muet / piste N) commun a toutes les fenetres pour la session, applique tout de suite a la lecture en cours.
- Linux : cpal demande ALSA (`libasound2-dev` / `alsa-lib-devel` pour compiler).
- La fenetre d'ecoute des pistes audio utilise encore ffplay (a basculer sur `PcmFeed` + cpal plus tard).
