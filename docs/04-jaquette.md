# 4. La jaquette et l'étiquette

L'onglet **Jaquette** contient deux documents : la **Jaquette** du boîtier (feuille à déplier : dos à gauche, tranche au milieu, face à droite) et l'**Étiquette de disque** (disque avec son trou central). On passe de l'un à l'autre en haut de l'onglet. Toutes les mesures sont en millimètres, ou en pouces si vous les affichez ainsi.

> 📷 **Capture à faire — `img/jaquette.png`** : onglet *Jaquette* complet : panneau de gauche (format, calques et éléments, inspecteur), feuille au centre avec dos, tranche et face, un titre, une image de vidéo, un tableau d'informations et un code-barres.

## Format et impression

**Jaquette**

- **Choisir un format usuel…** : Blu-ray standard (tranche 11 mm), Blu-ray épais (tranche 14 mm), DVD standard (tranche 14 mm), DVD slim (tranche 7 mm), Blu-ray slim (tranche 7 mm), boîtier cristal ;
- **Boîtier** : *standard* (feuille à déplier) ou *cristal* ; changer de boîtier reprend ses dimensions usuelles ;
- ou un format personnalisé : largeur du **dos / face**, **hauteur**, **tranche** ;
- **boîtier cristal** : la feuille porte deux pièces à découper, le **livret** (120 × 120 mm, sous le couvercle) en haut et le **verso** (150 × 118 mm, sous le plateau du disque) en bas, avec son **dos** (137 mm) entre deux **tranches** de 6,5 mm à plier. Leurs dimensions se règlent à part. Les deux pièces ont chacune leur fond perdu et leurs traits de coupe ; l'espace blanc entre elles est perdu à la découpe. « Placer sur » propose le livret, la tranche gauche, le dos et la tranche droite ;
- **fond perdu** : marge imprimée au-delà du format fini, rognée à la découpe ;
- **résolution** de l'export (dpi) ; la taille de la feuille en millimètres et en pixels est affichée ;
- **Traits de coupe et de pliage** : ajoutés à l'export, sur une page agrandie de 10 mm de chaque côté.

**Étiquette de disque** : **diamètre**, **trou central**, fond perdu, résolution, et **cercles de repère** (contour et trou) en option.

**Fond** : une couleur pour le dos, la tranche et la face (boîtier cristal : dos du verso, tranches et livret ; ou pour le disque), et une **image de fond** facultative qui couvre toute la feuille, fond perdu compris.

## Ajouter des éléments

| Bouton | Élément |
|---|---|
| **+ Texte** | texte avec champs automatiques |
| **+ Image…** | PNG, JPEG, SVG |
| **+ Image de vidéo** | une image de l'une des vidéos du projet, choisie image par image (même fenêtre que dans l'éditeur de menus) ; un double-clic sur l'élément ouvre cette fenêtre |
| **+ Tableau d'infos** | durée, langues, sous-titres, format et son des vidéos |
| **+ Code-barres** | Code 128, EAN-13, EAN-8 ou UPC-A |
| **+ Rectangle**, **+ Ellipse**, **+ Ligne** | formes |

## Calques et éléments

La liste du panneau de gauche va du premier plan (en haut) à l'arrière-plan (en bas). Comme dans l'éditeur de menus :

- **+ Calque** ; le **calque actif** reçoit les nouveaux éléments ;
- **verrouiller** un calque (plus de sélection à la souris) ;
- **masquer** un calque : un calque masqué n'est **ni affiché ni exporté** ;
- glisser ☰ pour réordonner ou changer de calque ; double-clic pour renommer.

> 📷 **Capture à faire — `img/jaquette-calques.png`** : liste des calques et éléments de la jaquette, avec un calque masqué et un calque verrouillé.

## Manipuler les éléments

- **Glisser** : déplacer ; **coins et côtés** : redimensionner (Maj : proportionnel) ; une ligne se manipule par ses deux extrémités (Maj : angle par pas de 45°).
- **Rectangle de sélection** ou **Maj+clic** : sélection multiple, puis alignement, centrage, répartition ou copie du groupe. Les réglages de l'élément principal restent affichés, et ce que vous y changez s'applique aussi aux autres éléments sélectionnés du même type (taille, rotation, couleurs, police, contour, ombre, bordure…), le contenu et la position de chacun restant les siens.
- **Flèches du clavier** : 0,5 mm (Maj : 5 mm, Ctrl : 0,1 mm) ; **Suppr** : supprimer.
- **Ctrl+C / Ctrl+X / Ctrl+V**, **Dupliquer**, **Ctrl+Z / Ctrl+Y**.
- **Rotation** : horizontal, 90° (texte qui se lit de haut en bas, typique d'une tranche) ou 270° (de bas en haut).
- **Placer sur** : remplit en un clic le dos, la tranche, la face, le haut ou le bas de la feuille, ou le disque. Placé sur la tranche, un texte est tourné automatiquement.
- **Alignement et centrage** : entre éléments ou sur la feuille.

### Repères et vue

- **Aimanter** : pendant un déplacement, les bords et centres s'aimantent sur les repères de la feuille (dos, tranche, face) et sur les autres éléments.
- **Grille** : pas réglable ; Alt la désactive le temps du geste.
- **Marge de sécurité**, **règles en mm ou en pouces**.
- **Fond** et **Repères** : couleurs autour de la feuille et des lignes de coupe et de pli dans l'éditeur (non imprimées).
- **Zoom** : molette, **Ajuster** (feuille entière), **100 % = taille réelle** (approximative, écran supposé à 96 dpi). Clic molette, clic droit maintenu ou Espace + glisser : déplacer la vue.

## Réglages des éléments

### Texte

> 📷 **Capture à faire — `img/jaquette-texte.png`** : inspecteur d'un texte justifié avec la liste « Insérer un champ automatique… » ouverte.

- **Texte**, et **Insérer un champ automatique…** :

  | Champ | Remplacé par |
  |---|---|
  | `{titre}` | titre du disque |
  | `{liste_videos}` | une ligne par vidéo : numéro, titre et durée |
  | `{nb_videos}` | nombre de vidéos |
  | `{duree_totale}` | durée totale des vidéos |
  | `{nb_chapitres}` | nombre total de chapitres manuels |
  | `{annee}` | année en cours |

- **Taille** (points), **couleur**, **alignement** gauche / centre / droite ;
- **Justifier** : le texte est coupé en lignes de la largeur du cadre et les mots sont espacés pour remplir chaque ligne ; la dernière ligne d'un paragraphe garde l'alignement choisi ;
- **Position verticale** (haut, milieu, bas), **interligne** ;
- **Ajuster au cadre** : réduit la taille du texte s'il déborde ;
- **police**, **gras**, **italique**, **contour**, **ombre** ;
- **édition directement sur la feuille** : double-cliquez sur un texte de la feuille (le mot double-cliqué est sélectionné). Vous tapez alors dans le rendu lui-même, avec sa police, sa taille, ses couleurs et sa mise en page : clic pour placer le curseur, glisser ou Maj+clic pour sélectionner, double-clic pour un mot, flèches, Début / Fin, Ctrl+A, copier / couper / coller, Entrée pour un retour à la ligne. Un champ automatique y apparaît avec sa valeur et se déplace et s'efface d'un bloc. Une barre au-dessus du cadre propose **Gras**, **Italique**, une **couleur**, une **taille** (en points) et **Effacer la mise en forme** pour la sélection ; **Terminer**, Échap ou un clic ailleurs arrêtent la saisie ;
- **mise en forme d'une partie du texte** : sélectionnez des mots sur la feuille ou dans la zone de saisie du panneau, puis, sur la ligne **Sélection**, **Gras**, **Italique**, une **couleur**, une **taille**, ou **Effacer la mise en forme** pour que la sélection reprenne les réglages du texte entier. Dans la zone de saisie du panneau, les couleurs et l'italique sont montrés tels quels et le gras est souligné. La mise en forme suit le texte quand vous le modifiez (ce que vous tapez prend le style du caractère qui précède), et un champ automatique mis en forme garde ce style une fois remplacé. Une partie plus grande agrandit la hauteur de sa ligne ; avec **Ajuster au cadre**, toutes les tailles sont réduites dans la même proportion. Le contour et l'ombre s'appliquent à tout le texte ;
- **fond** : une couleur qui remplit tout le cadre du texte, sous celui-ci, avec une **opacité** (100 % : opaque ; en dessous, la photo ou la couleur sous le texte transparaît) et des **coins arrondis** en option (redimensionnez le cadre pour régler la marge autour du texte ; le fond tourne avec le texte).

### Image de vidéo

Vidéo, instant et **Choisir l'image…** : fenêtre de choix image par image avec recadrage (voir [Les fenêtres de lecture](02-videos.md#les-fenêtres-de-lecture)). Sans recadrage, c'est le centre de l'image en 16:9, ajusté au cadre sans déformation.

### Tableau d'informations

> 📷 **Capture à faire — `img/jaquette-tableau.png`** : tableau d'informations de deux vidéos avec intitulés, grille et fond des cases, et son inspecteur.

- **Vidéos** : toutes celles du projet (**Toutes** revient à toutes, y compris celles ajoutées plus tard), ou une sélection dans l'ordre voulu (glisser ☰).
- **Informations**, dans l'ordre choisi (**Monter** / **Descendre**) : titre, durée, langues, sous-titres, format, son.
- **Intitulés** : une ligne ou une colonne avec le nom de chaque information.
- **Une ligne par information** : échange lignes et colonnes, pratique pour une seule vidéo (intitulé | valeur).
- Taille, couleurs, **fond des cases**, fond distinct des intitulés, **grille**, police.

Les valeurs viennent du projet : la durée et le format dès que la vidéo a été lue ; les langues des pistes audio et des sous-titres ; le son d'après les formats choisis (Dolby Digital 2.0 ou 5.1, LPCM…).

### Code-barres

> 📷 **Capture à faire — `img/jaquette-code-barres.png`** : code-barres EAN-13 au dos de la jaquette, avec son inspecteur.

- **Type** : Code 128 (texte libre : lettres, chiffres, ponctuation, sans accents), EAN-13, EAN-8, UPC-A.
- **Valeur** : pour EAN et UPC, tapez les chiffres sans le chiffre de contrôle (il est ajouté) ou avec (il est vérifié). Les champs automatiques sont acceptés.
- **Texte lisible** sous les barres, avec sa propre police et sa taille.
- **Couleur** des barres et **fond** : blanc par défaut, zones de silence comprises. Des barres sombres sur une jaquette sombre ne se lisent pas.

Le dessin est vectoriel et reste identique à tout zoom et à toute résolution. Laissez de la place autour du code et gardez une taille suffisante pour qu'il soit lisible par un lecteur de codes-barres.

### Formes

**Rectangle** et **Ellipse** : remplissage, bordure et épaisseur, coins arrondis (rectangle). **Ligne** : début et fin (X, Y), épaisseur, type de trait, flèches.

## Exporter et imprimer

- **Exporter en PDF…** : l'image à la résolution choisie, dans un PDF aux dimensions exactes.
- **Exporter en PNG…** : l'image à la résolution choisie, enregistrée dans le fichier. **PNG rogné au format fini (sans fond perdu)** en option.
- **Imprimer sur A4…** : un PDF A4 avec la jaquette à sa taille réelle, centrée, avec des repères de coupe. Imprimez-le **à 100 %**, sans « ajuster à la page », puis découpez.

Pour l'étiquette de disque, le PDF découpe le disque et son trou central.

L'export se fait en arrière-plan : vous pouvez continuer à travailler.
