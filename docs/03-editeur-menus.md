# 3. L'éditeur de menus

L'onglet **Mise en page** dessine les menus du disque. Le rendu affiché est celui qui sera gravé.

> 📷 **Capture à faire — `img/mise-en-page.png`** : onglet *Mise en page* complet : arborescence *Pages et calques* en haut à gauche, inspecteur d'un bouton sélectionné en bas à gauche, aperçu d'une page avec vignettes et boutons au centre, barre des éléments au-dessus.

La fenêtre a trois zones :

- **en haut à gauche, Pages et calques** : l'arborescence des pages, de leurs calques et de leurs éléments ;
- **en bas à gauche, l'inspecteur** : les réglages de l'élément sélectionné, ou de la page si rien n'est sélectionné (la séparation entre les deux se règle à la souris) ;
- **au centre, l'aperçu** de la page affichée, avec au-dessus la barre des éléments à ajouter.

## Premiers pas : les pages de départ

Un projet sans page affiche **Aucune page de menu** et propose **Créer les pages de départ** : une grille de vignettes des vidéos, puis une page de chapitres pour chaque vidéo qui en a. Tout reste ensuite modifiable.

Vous pouvez aussi partir d'une page vierge avec **+ Page**. Sans aucune page, la génération du disque crée elle-même ces pages de départ.

## Pages et calques

> 📷 **Capture à faire — `img/pages-calques.png`** : arborescence avec trois pages (l'une marquée 🏠), une page dépliée montrant deux calques et leurs éléments, un calque verrouillé et un calque masqué.

- **+ Page** ajoute une page vierge à la fin ; **+ Calque** ajoute un calque au premier plan de la page affichée.
- **+ tout / − tout** déplient ou replient toute l'arborescence.
- **Clic** sur une page : l'afficher ; **double-clic** : la renommer. **Glisser ☰** change l'ordre des pages.
- **🏠 Définir comme page d'accueil** (dans les réglages de la page) : page affichée à l'ouverture du disque et par la touche TOP MENU.
- **Supprimer la page** : les liens qui y menaient seront à rechoisir.

Les **calques** rangent les éléments d'une page :

- le **calque actif** reçoit les nouveaux éléments ;
- **verrouillé** 🔒, ses éléments ne se sélectionnent plus dans l'aperçu ;
- **masqué** dans l'aperçu, il reste sur le disque ;
- glisser un calque le déplace avec ses éléments, éventuellement vers une autre page ;
- glisser un élément change son ordre ou son calque, ou le dépose sur une autre page.

L'ordre des éléments est celui de la superposition : en haut de la liste, l'arrière-plan ; en bas, le premier plan.

## Ajouter des éléments

Glissez un élément de la barre sur la page pour le placer, ou cliquez dessus pour l'ajouter.

| Élément | Usage |
|---|---|
| **Texte** | texte libre, ou cliquable |
| **Image** | PNG, JPEG, BMP ou SVG, ajustée au cadre sans déformation ; cliquable en option |
| **Image de la vidéo** | une image d'une vidéo du projet, choisie par son instant, recadrable ; cliquable en option |
| **Vignette vidéo** | lance la vidéo depuis le début ; libellé sous l'image |
| **Vignette de chapitre** | lance la vidéo depuis un chapitre ou un instant libre |
| **Rectangle** | remplissage, bordure, coins arrondis, transparence ; même dessin que dans l'éditeur de jaquette |
| **Ligne** | trait plein, tirets, points ; pointes de flèche ; horizontale, verticale ou oblique |
| **Bouton de navigation** | Précédent, Suivant, Tout lire, ou « Aller à… » une page, une vidéo, un réglage |

Une page contient au plus **12 vignettes**.

## Sélectionner, déplacer, aligner

- **Clic** sur un élément pour le sélectionner ; **Maj+clic** ajoute ou retire un élément de la sélection ; glisser l'un d'eux déplace tout le groupe.
- **Modifier plusieurs éléments à la fois** : avec une sélection multiple, l'inspecteur affiche les réglages de l'élément principal (le dernier cliqué), et ce que vous y changez s'applique aussi aux autres éléments sélectionnés **du même type** (par exemple la couleur, le style ou la police de tous les boutons). La taille, le style, les couleurs, la police, les images d'états et les sons passent à tous ; la position, le nom, le texte, la vidéo, le chapitre, l'image et la destination restent ceux de chaque élément. Seul le réglage modifié est reporté : changer l'arrondi ne touche pas aux couleurs propres à chaque bouton.
- **Coins et côtés** du cadre : redimensionner (Maj : proportionnel).
- **Flèches du clavier** : déplacer d'un pixel (Maj : 10 pixels) ; **Suppr** : supprimer.
- **Ctrl+C / Ctrl+X / Ctrl+V** : copier, couper, coller, y compris d'une page à l'autre ; **Dupliquer** dans l'inspecteur.
- **Alignement et centrage** (plusieurs éléments) : aligner à gauche, au centre, à droite, en haut, au milieu, en bas ; centrer sur la page ; répartir horizontalement ou verticalement.
- **Aimantation** : **Aligner sur les autres éléments** aimante les bords et centres sur ceux de l'écran et des autres éléments ; **Aimantation sur une grille de** *n* pixels.
- **Vue** : molette pour zoomer ; clic molette, clic droit maintenu ou Espace + glisser pour déplacer la vue ; **Ajuster** pour revenir à la page entière.

## L'inspecteur

> 📷 **Capture à faire — `img/inspecteur-vignette.png`** : inspecteur d'une vignette de chapitre : vidéo, chapitre lié, libellé, « Au clic », « Après la lecture », sous-titres, navigation.

### Réglages communs

- **Nom** (facultatif) : pour s'y retrouver dans l'arborescence et les listes.
- **X, Y, Largeur, Hauteur**, en pixels d'un menu 1920 × 1080.
- **Cliquable (devient un bouton du menu)**, pour un texte ou une image. Sélectionné à la télécommande, l'élément est entouré d'un cadre.

### Selon l'élément

- **Texte** : texte, taille, couleur, police (par défaut, une police installée, du dossier de polices supplémentaires, ou **Autre fichier…**), alignement, fond (opacité, coins), contour du texte.
- **Image** : fichier.
- **Image de la vidéo** : vidéo, instant, **Choisir l'image…** pour la choisir image par image et la recadrer.
- **Vignette vidéo / de chapitre** : vidéo ; départ **Depuis le début**, **Chapitre** (lié à un chapitre de la vidéo : la vignette suit son instant) ou **Instant libre** ; **Créer le chapitre** ajoute un chapitre à cet instant ; **Détacher** rompt le lien ; **Libellé** ; **Image de la vignette prise à un autre instant** ; hauteur du libellé.
- **Rectangle** : remplissage, bordure, épaisseur, coins arrondis, opacité.
- **Ligne** : épaisseur, style de trait, pointes de flèche, boutons pour la rendre horizontale ou verticale.

### Ce que fait le bouton

**Au clic** (ou **Destination** pour un bouton de navigation « Aller à… ») :

| Action | Effet |
|---|---|
| **Lire la vidéo** | depuis le début, ou depuis un chapitre ou un instant pour une vignette de chapitre |
| **Page** | affiche une autre page du menu |
| **Tout lire** | toutes les vidéos à la suite (bouton de navigation) |
| **Précédent / Suivant** | page précédente ou suivante (bouton de navigation) |
| **Régler les sous-titres** | une langue de la liste, ou **Masquer les sous-titres** ; **Ensuite** : rester sur la page ou passer à une page |
| **Régler la langue audio** | une langue de la liste ; **Ensuite** : rester sur la page ou passer à une page |

Le réglage de langue audio vaut pour toutes les vidéos lues ensuite. Une vidéo qui n'a pas cette langue garde sa piste. Pour les sous-titres, la langue correspond au même numéro de piste dans toutes les vidéos (voir [Les langues du projet](02-videos.md#les-langues-du-projet)).

**Après la lecture** (vignettes) : **Revenir à cette page**, **Aller à la page d'accueil** ou **Aller à la page** *n*.

**Sous-titres** : un bouton qui lance une vidéo peut aussi régler les sous-titres juste avant la lecture.

### Navigation à la télécommande

Par défaut, chaque flèche va au bouton le plus proche dans sa direction. Dans **Navigation (flèches de la télécommande)**, chaque direction (↑ Haut, ↓ Bas, ← Gauche, → Droite) peut viser un autre bouton de la page, ou **Aucun (reste sur place)**. À l'ouverture d'une page, le premier bouton (dans l'ordre des éléments) est sélectionné.

### Apparence et sons propres

Par défaut, les boutons suivent le **style des boutons du projet** (voir [chapitre 5](05-projet-et-generation.md#style-des-boutons)). **Apparence propre à cet élément** (couleurs, cadre, arrondi, ombre, halo, police du texte…) et **Sons propres à cet élément** le remplacent pour un seul bouton.

## Réglages de la page

Cliquez dans une zone vide de l'aperçu pour régler la page affichée.

> 📷 **Capture à faire — `img/reglages-page.png`** : inspecteur de la page : fond et animations, lancement automatique réglé à 30 s, page d'accueil.

- **Fond et animations de cette page** : **Comme le projet**, ou une **image** ou une **vidéo** propre à la page (jouée en entier, en boucle ; **Garder le son de la vidéo** s'il n'y a pas de musique), et une **musique** propre. Chaque fond différent devient un clip sur le disque, encodé une fois puis gardé en cache ; sa durée et sa place estimée sont affichées.
- **Animations des boutons** : cadence en images par seconde, une seule par page (norme Blu-ray), pour le halo animé et les animations d'images.
- **Lancement automatique** : **Lancer un élément si rien n'est pressé** au bout de *n* secondes, par exemple lancer le film quand personne ne touche à la télécommande. Le délai est arrondi à un nombre entier de passages du fond.
- **🏠 Définir comme page d'accueil**.

## Signalements dans l'éditeur

- **Au-dessus de l'aperçu**, les problèmes de la page affichée : **Erreur** (bloquant pour la génération) ou **Attention**. Par exemple : destination manquante, élément hors de l'écran, vignette sans vidéo, page sans sortie.
- **Dans l'aperçu**, un clignotement rouge signale des boutons qui se chevauchent ; orange, un texte caché par un bouton.
- **Poids des boutons** : la mémoire d'images que le lecteur doit décoder pour afficher la page (16 Mo au plus). Au-delà, le menu risque de ne pas s'afficher sur certains lecteurs ; allégez les halos, les agrandissements, les animations ou le nombre de boutons.

## Simuler la télécommande

**▶ Simuler la télécommande** parcourt le menu comme sur un lecteur :

- **flèches** : passer au bouton voisin, avec la même règle que sur le disque ;
- **Entrée** ou clic : valider ; l'état « activé » s'affiche un instant ;
- **M** : touche TOP MENU, retour à la page d'accueil ;
- **Échap** : arrêter la simulation.

Les lectures de vidéo sont simulées par un écran noir ; à la fin, le menu revient là où le disque reviendrait. Les réglages de langue et de sous-titres sont annoncés.

> 📷 **Capture à faire — `img/simulateur.png`** : simulation en cours, bandeau « Simulation · bouton 2 sur 5 … » visible, un bouton sélectionné.

## Vérifier les menus

**Édition › Vérifier les menus…** examine le menu comme un tout : pages, boutons, vidéos et retours.

> 📷 **Capture à faire — `img/verifier-menus.png`** : fenêtre *Vérifier les menus* avec quelques constats (erreur, avertissements, infos) et la carte du disque.

En haut, le nombre d'**erreurs**, d'**avertissements** et d'**infos** ; cliquez sur l'un d'eux pour n'afficher que ces constats. La liste se met à jour pendant que vous modifiez le projet. **Un clic sur un constat** ouvre la page dans l'éditeur avec l'élément sélectionné, ou sélectionne la vidéo dans l'onglet *Vidéos*.

| Règle | Ce qui est signalé |
|---|---|
| **Accessibilité** | une page vers laquelle aucun bouton ne mène depuis l'accueil |
| **Vidéos orphelines** | une vidéo qu'aucun bouton ne lance, ou seulement depuis des pages inaccessibles (« Tout lire » compte pour toutes les vidéos) |
| **Retour après lecture** | une vignette qui revient à une page supprimée |
| **Lancement automatique** | des pages qui se lancent l'une l'autre sans fin ; si la boucle lit une vidéo, c'est signalé comme une information (boucle de démonstration ?) |
| **Navigation aux flèches** | un bouton impossible à atteindre avec les flèches depuis le bouton sélectionné à l'ouverture ; une flèche réglée à la main sans retour |
| **Chapitres** | une vignette dont le chapitre lié a disparu ; un départ après la fin de la vidéo (erreur) |
| **Langues** | un réglage audio ou de sous-titres sans piste dans certaines vidéos |
| **Doublons** | deux boutons de la même page qui font exactement la même chose |

En bas, la **carte du disque** résume chaque page (🏠 l'accueil) avec ses sorties, et chaque vidéo avec les pages qui la lancent et où l'on revient. Les pages inaccessibles sont en rouge, les vidéos jamais lancées en orange.

Les erreurs et avertissements sont aussi écrits en tête du journal de génération, sans la bloquer.
