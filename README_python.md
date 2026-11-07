# bdauthor : creation de menus Blu-ray (HDMV)

Installation (Windows) :
1. Extraire cette archive dans `C:\Users\moyla\Desktop\Authoring\code` : on doit y voir les dossiers `bdauthor`, `tests`, `examples` et `NOTES.md`.
2. Python 3.10 ou plus recent ; dans une invite de commandes : `cd C:\Users\moyla\Desktop\Authoring\code` puis `python -m pip install -r requirements.txt`.
3. Toutes les commandes se lancent depuis ce dossier (celui qui contient `bdauthor` et `tests`).

Exemples :
    python -m tests.build_menu_png                                   (boutons PNG -> out\menu_png_overlay.zip)
    python -m bdauthor.disc chemin\vers\00000.m2ts sortie            (dossier BDMV complet depuis un film .m2ts)
    python -m tests.gen_clpi 00000.m2ts 00000.clpi                   (regenere le .clpi et le compare a une reference)

Les dossiers `out\` et les sorties sont crees automatiquement. Lire NOTES.md pour l'etat du projet, ce qui est verifie et ce qui ne l'est pas.
Supprimer de l'ancien projet : rle_menu.py et igs_muxer.py (remplaces par ig.py, mux.py, png_palette.py).


## Disque multi-titres a partir de vos videos
1. Installer ffmpeg (avec libx264 et l'encodeur AC3) et verifier dans une invite de commandes : `ffmpeg -encoders` doit
   lister `libx264` et `ac3`. Le dossier `bin` de ffmpeg doit etre dans le PATH de Windows.
2. Copier `examples\projet_exemple.json` (le modifier : chemins des videos, titres). Les chemins peuvent etre relatifs au fichier JSON ;
   avec des barres obliques `/` (ou doubler les barres inverses `\\`).
3. Lancer : `python -m bdauthor.project projet.json sortie` (compter de quelques secondes a quelques minutes par video).
4. Verifier le resultat sans lecteur : `python -m bdauthor.check sortie\BDMV`, puis ouvrir `sortie` dans VLC.

Options du projet : `titre` (affiche en haut du menu), `langue` (code ISO a 3 lettres de la piste audio), `videos` (1 a 12 ;
`vignette_s` = instant de l'image de la vignette), `encodage` (`crf` : plus petit = meilleure qualite, `preset` : de `ultrafast` a `slow`),
`menu` (`fond` : une image PNG ou JPG a la place du degrade bleu, `fond_secondes` : duree du clip de fond, `taille` : `video` ou `1080`).
Toutes les videos devraient avoir la meme resolution et la meme cadence que la premiere.

Chapitres : par video, `"chapitres": [0, 120, {"t": 300, "titre": "Le pont"}]` (secondes ; le 0 est ajoute si absent) ou
`"chapitres_auto_s": 60` (un chapitre toutes les 60 s). De 2 a 12 chapitres : des reperes sont poses dans la playlist (touches chapitre
suivant / precedent) et le bouton de la video, dans le menu principal, ouvre un menu de chapitres (vignettes, « Tout lire », « Retour »).
Chaque repere est cale sur l'image d'acces qui precede l'instant demande (le chapitre demarre donc un peu avant, au plus ~0,5 s avec la
valeur par defaut d'une image d'acces toutes les 30 images). Une video sans `chapitres` se lance directement.
