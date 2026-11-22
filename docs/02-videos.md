# 2. Les vidéos

L'onglet **Vidéos** décrit le contenu du disque. À gauche, la liste des vidéos ; à droite, les réglages de la **vidéo sélectionnée**.

> 📷 **Capture à faire — `img/videos-onglet.png`** : onglet *Vidéos*, trois vidéos dans la liste, la deuxième sélectionnée ; à droite, titre, vignette, chapitres, pistes audio et sous-titres visibles.

## Ajouter et organiser les vidéos

- **Ajouter des vidéos…** ouvre un sélecteur de fichiers (sélection multiple possible). Formats acceptés : mp4, mkv, mov, avi, m2ts, mts, ts, webm, wmv, flv, m4v, mpg, mpeg.
- **Glisser la poignée ☰** d'une vidéo change l'ordre. C'est l'ordre du disque : celui des pages de départ et de « Tout lire ».
- **Recharger** (bouton à côté de la vidéo) relit la durée, les pistes et les vignettes, par exemple après avoir remplacé le fichier.
- **Retirer** enlève la vidéo du projet après confirmation. Ses réglages (chapitres, pistes audio, sous-titres) sont perdus, mais le fichier vidéo n'est pas supprimé.

Une vidéo dont le fichier est introuvable apparaît avec « (inconnue) ».

## Titre et vignette

- **Titre dans le menu** : le nom affiché sous la vignette dans les menus (par défaut, le nom du fichier).
- **Vignette alternative** : décochée, l'image de la vignette est prise automatiquement ; cochée, elle est prise à l'instant choisi. **Choisir l'image…** ouvre la fenêtre de choix d'image (voir [Les fenêtres de lecture](#les-fenêtres-de-lecture)).

## Chapitres

Les chapitres servent aux touches *chapitre suivant / précédent* de la télécommande, aux vignettes de chapitre des menus et aux pages de départ.

Chaque chapitre a un **instant** au format `hh:mm:ss:ii` (ii = numéro de l'image dans la seconde) et un **nom** facultatif. On peut glisser la valeur, ou taper un timecode ou un nombre de secondes. **Ajouter un chapitre** en crée un ; le bouton de retrait le supprime.

Un compteur « n vignettes » indique les vignettes de pages liées à ce chapitre : elles suivent son instant. Si vous retirez le chapitre, elles gardent leur instant.

> Les anciens projets peuvent utiliser des repères automatiques (un chapitre toutes les N secondes). bdauthor l'indique et propose **Les retirer** pour passer à des chapitres choisis.

### L'éditeur de chapitres

**Éditeur de chapitres…** ouvre une fenêtre avec la vidéo image par image, la lecture avec le son et une frise de vignettes.

> 📷 **Capture à faire — `img/editeur-chapitres.png`** : éditeur de chapitres avec quatre chapitres sur la frise, le deuxième sélectionné, la liste des chapitres à droite et la partie *Vignettes*.

- **Se déplacer** : boutons −10 s, −1 s, ◀ image, image ▶, +1 s, +10 s ; clavier : ← → (une image), avec Maj (1 s) ou Ctrl (10 s).
- **Lecture / pause** : bouton ou Espace.
- **+ Chapitre ici (C)** ajoute un chapitre à l'image affichée.
- **Sur la frise** : clic pour aller à un instant, glisser un repère pour déplacer un chapitre, molette pour zoomer, Maj + molette pour défiler.
- **◀ Image clé / Image clé ▶** saute aux images clés de la vidéo source (là où un lecteur peut démarrer sans décoder les images précédentes).
- **Image clé exacte sur chaque chapitre** : la vidéo est encodée avec une image clé sur chaque chapitre, et le lecteur démarre exactement sur l'image choisie.
- **Vignettes** : choisissez l'image de la vignette de la vidéo (**Utiliser l'image affichée** ou *automatique*) et, pour un chapitre sélectionné, l'image des vignettes de page qui lui sont liées.

**Valider** applique les modifications, **Annuler** les abandonne.

## Pistes audio

Chaque vidéo peut avoir **jusqu'à 8 pistes audio**, choisies avec la télécommande. Sans piste choisie, la première piste audio du fichier est utilisée, dans la langue audio par défaut du projet (**Édition › Projet…**).

> 📷 **Capture à faire — `img/pistes-audio.png`** : section *Pistes audio* d'une vidéo avec trois pistes (deux intégrées, une venant d'un fichier), les boutons 🔊 et ✏ visibles.

- **Pistes audio de la vidéo…** liste les pistes présentes dans le fichier (langue, format, nombre de canaux) ; **Ajouter** les place dans la liste.
- **Ajouter un fichier audio…** prend une piste dans un autre fichier (wav, flac, mp3, ac3, eac3, dts, aac, m4a, ogg, opus, mka…), par exemple un doublage séparé.
- **Glisser ☰** change l'ordre : la piste 1 est en haut.
- **Langue** : chaque piste choisit sa langue dans la liste des langues du projet (onglet *Langues des sous-titres* de **Édition › Projet…**).
- **🔊** ouvre la fenêtre d'écoute de la piste : forme d'onde, **Lecture / Pause**, **Arrêter** (retour au début), clic ou glisser sur la frise pour se déplacer. La **molette** zoome autour du pointeur (jusqu'à une seconde visible, avec une forme d'onde recalculée finement pour la portion affichée) ; **Maj + molette** se déplace dans la piste, et **Toute la piste** revient à la vue complète. Pendant la lecture, un **vumètre** montre le niveau de crête de chaque canal en dBFS (vert, puis jaune au-dessus de −18 dB, rouge au-dessus de −6 dB), avec le pic maintenu et un voyant rouge en cas de saturation. Pratique pour caler un décalage.

### Réglages d'une piste (✏)

| Réglage | Rôle |
|---|---|
| **Fichier** | pour une piste venant d'un fichier séparé |
| **Décalage +/- (ms)** | positif : le son est retardé ; négatif : il est avancé. Sert à resynchroniser une piste avec l'image. |
| **Convertir au format** | format de la piste sur le disque (voir ci-dessous) |
| **Débit** | pour le Dolby Digital encodé : de 32 à 640 kb/s ; *défaut du projet* (stéréo) ou 448 kb/s (5.1) |

> 📷 **Capture à faire — `img/piste-audio-format.png`** : réglages d'une piste avec la liste « Convertir au format » ouverte.

Formats disponibles :

| Format | Usage | Débit |
|---|---|---|
| **Dolby Digital 2.0** | par défaut, compact | réglable, défaut du projet |
| **Dolby Digital 5.1** | son multicanal compact ; une source de plus de 6 canaux est mixée en 5.1 | réglable, 448 kb/s par défaut |
| **Copie de la piste AC-3 de la source** | garde telle quelle une piste Dolby Digital existante, sans réencodage | celui de la source |
| **LPCM 16 bits / LPCM 24 bits** | son sans perte, en 48 ou 96 kHz | imposé : environ 4,6 Mb/s (5.1, 16 bits), 6,9 Mb/s (5.1, 24 bits) à 48 kHz ; le double à 96 kHz |

À savoir :

- **Copie** : la piste source doit être en Dolby Digital, à 48 kHz, avec 6 canaux au plus et 640 kb/s au plus. Sinon la génération s'arrête avec un message explicite.
- **Dolby Digital 5.1 depuis une source stéréo** : seuls les canaux avant sont remplis ; ce choix n'apporte rien.
- **LPCM** : le débit de la vidéo est automatiquement abaissé pour que vidéo et son tiennent ensemble. Tous les lecteurs n'acceptent pas le LPCM : essayez le disque sur le vôtre. Une source 5.0 ou 6.1 est convertie en 7.0 ou 7.1 par ffmpeg. Le réglage **Fréquence** choisit 48 kHz (par défaut) ou 96 kHz. Le 96 kHz ne sert qu'avec une source haute résolution (musique, concert) : il double la taille de la piste, et avec une source à 48 kHz il n'apporte rien (l'appli le signale). Une source à 192 kHz est ramenée à la fréquence choisie.

Le réglage **Canaux** allège une piste LPCM : **comme la source** (par défaut), **5.1 au plus** (une source 7.1, 7.0 ou 6.1 est mixée en 5.1, les canaux arrière allant dans les canaux surround) ou **stéréo** (centre et surrounds ajoutés à gauche et à droite, caisson de basses gardé), avec l'option **Dolby Pro Logic II** : une stéréo ordinaire dont un ampli home-cinéma peut retirer un effet surround. Une source qui a déjà moins de canaux n'est pas touchée. À côté s'affichent les canaux de la source et le débit obtenu : en 24 bits, une piste 7.1 passe de 9,2 Mb/s à 6,9 Mb/s en 5.1 et à 2,3 Mb/s en stéréo. Un mixage stéréo est environ 5 dB plus faible que l'original sur les dialogues.
- Le LPCM ne rend pas meilleure une source déjà compressée (AC-3, AAC, MP3) : il la conserve fidèlement, en plus gros. Il garde en revanche toute la qualité d'une source sans perte (FLAC, WAV, TrueHD, DTS-HD MA décodés).

## Sous-titres

Chaque vidéo peut avoir **jusqu'à 8 pistes de sous-titres**, que le lecteur affiche au choix avec la télécommande. Les sous-titres texte sont dessinés en images (format PGS du Blu-ray) avec la police et le style de la piste.

- **Ajouter un fichier de sous-titres…** : SRT, ASS/SSA, WebVTT, SAMI, MicroDVD, SubViewer, TTML et d'autres formats texte.
- **Pistes intégrées à la vidéo…** extrait une piste de sous-titres texte du fichier vidéo. Les sous-titres en images (DVD, PGS) ne sont pas gérés.
- **Glisser ☰** change l'ordre ; **langue** à choisir dans la liste du projet.
- **✏** ouvre la fenêtre d'aperçu de la piste avec ses réglages.

### La fenêtre d'aperçu des sous-titres

> 📷 **Capture à faire — `img/apercu-sous-titres.png`** : aperçu sur l'image de la vidéo, une réplique de deux lignes affichée, la zone de sécurité visible, les réglages de la piste à droite.

À gauche, l'aperçu de la réplique choisie, sur un fond **Noir**, **Gris**, **Clair** ou sur l'**Image de la vidéo**, avec la **Zone de sécurité** en option. En dessous, la lecture de la vidéo avec le son et la frise, où les répliques apparaissent en repères. Les boutons **Réplique précédente**, **Réplique suivante** et **Plus longue** (la plus exigeante pour la mise en page) aident à vérifier le rendu.

À droite, les réglages, que **Détacher** place dans leur propre fenêtre :

**Réglages de la piste**

| Réglage | Rôle |
|---|---|
| **Encodage du fichier** | *Automatique* détecte l'encodage (UTF-8, pages de code Windows, cyrillique, grec, hébreu, arabe, thaï, japonais, chinois, coréen…) ; à forcer si des caractères sont faux |
| **Cadence** | pour les fichiers MicroDVD, qui comptent en images : la cadence de la vidéo d'origine |
| **Décalage +/- (ms)** | avance ou retarde toutes les répliques |
| **Retirer les indications [entre crochets] et (entre parenthèses)** | pour les sous-titres pour sourds et malentendants |
| **Garder l'italique, le gras et les couleurs du fichier** | dessine les balises du fichier |
| **Respecter les positions du fichier** | répliques placées en haut, à gauche, etc. (ASS) |

**Style de la piste** : police, taille, couleur, contour et sa couleur, ombre, opacité, boîte derrière le texte (opacité, coins), position, marges, alignement, largeur maximale, et **Couper les lignes longues** (sinon la taille est réduite pour que tout tienne). Les tailles sont données pour une vidéo de 1080 lignes et s'adaptent à 720.

**Contrôle de qualité** : vitesse de lecture (caractères par seconde), longueur de ligne, nombre de lignes, durée d'affichage et intervalle minimal entre répliques, avec des limites réglables (**Valeurs usuelles** pour revenir aux valeurs habituelles). Les répliques en défaut sont listées.

## Les langues du projet

La liste des langues se règle dans **Édition › Projet…**, onglet *Langues des sous-titres*. Elle sert aux pistes audio, aux sous-titres et aux boutons « Régler la langue audio » ou « Régler les sous-titres » des menus.

- **Ajouter une langue ▾** propose les langues courantes, ou **Autre…** pour un nom et un code de 3 lettres.
- **Pour les sous-titres, le rang dans la liste fixe le numéro de piste**, identique dans toutes les vidéos : c'est ce qui permet à un bouton « sous-titres en anglais » de fonctionner pour toutes les vidéos.

> 📷 **Capture à faire — `img/langues-projet.png`** : onglet *Langues des sous-titres* avec trois langues.

## Les fenêtres de lecture

Trois fenêtres montrent la vidéo image par image avec la lecture : le choix d'une image (vignette, « Image de la vidéo » des menus et de la jaquette), l'éditeur de chapitres et l'aperçu des sous-titres.

> 📷 **Capture à faire — `img/choix-image.png`** : fenêtre *Image de la vidéo* avec le cadre de recadrage, la frise et le menu 🔊 du son.

- **Lecture avec le son** : le menu **🔊** à côté du bouton de lecture choisit **Muet** ou l'une des pistes audio de la vidéo. Le choix vaut pour toutes les fenêtres et s'applique tout de suite. L'image suit le son ; sans sortie audio, la lecture continue en muet.
- **Frise** : survol pour un aperçu, clic ou glisser pour choisir l'instant, molette pour zoomer, Maj + molette pour défiler.
- **Recadrer l'image** (fenêtre de choix d'image) : proportions libres ou celles du widget, **Plus grand cadre** ; glissez le cadre, ses angles ou ses côtés, ou tracez-en un nouveau. Sans recadrage, c'est le centre de l'image en 16:9.

Les vignettes des frises sont calculées en arrière-plan et gardées dans un cache sur le disque dur : les fenêtres s'ouvrent ensuite aussitôt.
