# 5. Réglages du projet et génération du disque

## Les réglages du projet

**Édition › Projet…** ouvre les réglages enregistrés avec le projet, répartis en cinq onglets.

> 📷 **Capture à faire — `img/projet-general.png`** : fenêtre *Projet*, onglet *Général* (Disque et Encodage).

### Général

**Disque**

- **Titre du disque** : affiché par les lecteurs et utilisé par les champs automatiques de la jaquette.
- **Langue audio par défaut** (3 lettres, par exemple `fra`) : langue de la piste audio des vidéos dont les pistes ne sont pas choisies.
- **Séquence d'ouverture** : une vidéo lue à l'insertion du disque, avant le menu. Sans elle, le menu s'ouvre directement.
- **Taille du menu** : 1920 × 1080 (recommandé), taille de la première vidéo, ou personnalisée. Le menu est toujours en 16:9. Un format autre que 1920 × 1080 ou 1280 × 720 n'est pas standard et peut être refusé par certains lecteurs.

**Encodage**

- **Qualité x264 (CRF)** : plus petit = meilleure qualité et fichiers plus gros (18 à 22 est un bon début).
- **Préréglage d'encodage** : plus lent = plus compact à qualité égale.
- **Viser un débit qui remplit le disque** : au lieu d'une qualité fixe, les vidéos à encoder se partagent ce qui reste du disque une fois comptés les menus, l'audio et les vidéos copiées, avec le même débit moyen pour toutes. **Disque** visé et part de la capacité à **remplir jusqu'à** (c'est aussi la marge de sécurité de l'image ISO).
- **Copier les vidéos déjà conformes au Blu-ray** : une vidéo H.264 déjà admise sur un Blu-ray (profil, niveau, taille, cadence, débit) garde son flux vidéo tel quel ; c'est plus rapide et sans perte.
- **Normaliser le volume des pistes audio** : toutes les vidéos du disque sont ramenées au même volume (−23 LUFS, filtre `loudnorm` de ffmpeg), pour ne pas toucher à la télécommande entre deux films. Chaque piste est d'abord mesurée (une lecture rapide de l'audio seul, sautée quand l'encodage est déjà en cache), puis corrigée d'un gain constant : la dynamique est conservée. Dans le panneau d'une piste audio, **Normaliser le volume** vaut *Comme le projet*, *Oui* ou *Non* ; la normalisation remplace alors le niveau en dB. Sans effet sur une piste recopiée telle quelle (AC-3, DTS, TrueHD). Si la mesure échoue (piste muette), la normalisation se fait en une seule passe.
- **Réutiliser les vidéos déjà encodées (cache)** : une vidéo encodée avec les mêmes réglages n'est pas réencodée à la génération suivante.

### Style des boutons

L'apparence de tous les boutons du projet, sauf ceux qui ont leur propre apparence (voir l'inspecteur, [chapitre 3](03-editeur-menus.md#apparence-et-sons-propres)).

> 📷 **Capture à faire — `img/projet-style-boutons.png`** : onglet *Style des boutons*, avec l'aperçu d'un bouton sélectionné à halo.

| Partie | Réglages |
|---|---|
| **Texte des boutons** | police (par défaut, une police installée ou du dossier de polices supplémentaires), gras, italique ; pour les libellés des vignettes et le texte des boutons de navigation (les textes cliquables gardent leur propre police) |
| **Bouton non sélectionné** | fond, texte, cadre, assombrir |
| **Bouton sélectionné** | fond, texte, cadre, **halo** (et **halo animé** : pulsation en boucle), **agrandir** |
| **Commun** | arrondi des coins, **ombre portée** (distance et opacité : ombre décalée vers le bas à droite et floutée), état « activé » distinct (au moment de l'appui sur OK) |
| **Textes et images cliquables** | fond coloré (opacité), souligner au lieu d'encadrer |
| **Sons** | son à la sélection et à l'appui sur OK : sons courts, moins d'une seconde |
| **Images dessinées** (facultatif) | une image par état (normal, sélectionné, activé) qui remplace le dessin du bouton ; plusieurs images = une animation (en boucle ou une fois) |

**Valeurs par défaut** remet les couleurs, le cadre et le halo d'origine (les sons sont gardés). Les tailles sont données pour un menu 1920 × 1080 : le halo, l'ombre et l'agrandissement élargissent l'image des boutons, laissez de l'espace entre eux.

### Style des pages : fond et musique des menus

Valeurs par défaut des pages (chaque page peut avoir les siennes, voir [Réglages de la page](03-editeur-menus.md#réglages-de-la-page)) :

- **Image de fond** (sinon un fond généré automatiquement) et **durée du fond** ;
- **Vidéo de fond (animée)**, et **Garder le son de la vidéo de fond** s'il n'y a pas de musique ;
- **Musique des menus** ;
- **Rejouer le fond en boucle** (sinon arrêt sur la dernière image).

Le fond dure autant que la vidéo de fond ou, sans vidéo, que la musique.

### Langues des sous-titres

La liste ordonnée des langues du disque, pour les pistes audio et les sous-titres (voir [Les langues du projet](02-videos.md#les-langues-du-projet)).

### Génération ISO

> 📷 **Capture à faire — `img/projet-iso.png`** : onglet *Génération ISO*.

**Sortie** : la génération produit

- **le dossier BDMV seul** (dossier `BDMV` + `CERTIFICATE`, à graver avec un autre logiciel ou à lire tel quel) ;
- **l'image ISO seule** (le disque est construit dans un dossier temporaire, supprimé une fois l'image écrite) ;
- **le dossier et l'image ISO** (l'image est écrite à côté du dossier, même nom avec `.iso`, sauf si un fichier est indiqué).

**Volume** : **nom du volume** (affiché à l'insertion du disque ; vide = titre du disque), et informations facultatives : propriétaire, organisation, contact.

**Disque visé** : **capacité** (BD-25, BD-50, BD-100) et **marge de sécurité** : un avertissement apparaît quand l'image dépasse ce pourcentage du disque, car la gravure est moins fiable tout au bord d'un BD-R.

**Version UDF** : **2.50 (recommandé)**, la norme des Blu-ray, lue par toutes les platines.

**Disposition** :

- **Aligner les vidéos sur 64 Ko** : chaque fichier vidéo commence au début d'un bloc de correction d'erreurs du disque ;
- **Ordonner les fichiers selon la lecture** : navigation et menus en tête, puis les vidéos dans l'ordre ;
- **Compatibilité maximale** : structures de secours supplémentaires du système de fichiers ;
- **Copie miroir des métadonnées** : l'arborescence est écrite deux fois, à deux endroits du disque.

**Contrôle** :

- **Date inscrite dans l'image** : date et heure de la génération, date fixe (le même disque donne alors exactement la même image) ou date choisie ;
- **Vérifier l'image après création** : relit toute l'image et la compare au dossier, fichier par fichier. Recommandé avant de graver un BD-R.

## Générer le disque

**Édition › Générer le disque** ouvre la fenêtre de génération ; **Générer le disque…** la lance.

> 📷 **Capture à faire — `img/generation.png`** : fenêtre *Générer le disque* pendant un encodage : barre de progression, étape, temps écoulé et restant, ligne ffmpeg et journal.

1. Le projet est **enregistré**, puis **contrôlé** : s'il y a des erreurs bloquantes, elles s'affichent sous « Corrigez d'abord : » et rien n'est lancé.
2. bdauthor demande le **dossier de sortie**, ou le **fichier de l'image ISO**, selon l'onglet *Génération ISO*.
3. La génération encode les vidéos, les fonds de menu et les sous-titres, puis écrit les menus, la navigation et, le cas échéant, l'image ISO.

Pendant la génération :

- la barre de progression couvre tout, disque puis image ISO ;
- l'**étape en cours**, le **temps écoulé** et la **fin estimée** ;
- une ligne **ffmpeg** : position dans la vidéo, images par seconde, vitesse, temps restant de cet encodage ;
- le **journal** en direct. En tête, les erreurs et avertissements du [vérificateur des menus](03-editeur-menus.md#vérifier-les-menus) ;
- **Annuler** arrête la génération.

À la fin, un résumé (nombre de vidéos et de pages, format, taille des flux) et le résultat de la **vérification** du disque produit : « aucune erreur » en vert, ou les erreurs détectées en rouge. Le vérificateur intégré contrôle la cohérence du dossier BDMV (index, playlists, clips, menus) et de l'image ISO.

> **Commandes ffmpeg complètes** : pour diagnostiquer un encodage, cochez dans **Fichier › Configuration…** « Afficher les commandes ffmpeg complètes dans le journal de génération ». Chaque commande est alors écrite après un signe ».

## Tester et graver

- **Tester** : ouvrez le dossier produit dans un logiciel de lecture Blu-ray (VLC, par exemple) pour vérifier la lecture et les menus. Pour un test complet, gravez un **BD-RE** (réinscriptible) et essayez-le sur votre lecteur de salon.
- **Graver** : sous Windows, quand la génération a produit une image ISO, le bouton **Graver…** apparaît sous le résultat, avec la taille de l'image et le disque sur lequel elle tient (BD-25, BD-50, BD-100). Il ouvre le *Graveur d'images disque* de Windows, qui demande le graveur, propose de vérifier le disque après gravure et grave le BD-R ou le BD-RE. Pour une image déjà faite, **Fichier › Graver une image ISO…**. Ailleurs que sous Windows, gravez l'image avec votre logiciel de gravure habituel.

Les disques produits sont **sans zone et sans protection** (ni AACS, ni BD+, ni code de région) : ils sont lisibles sur tout lecteur. La protection anti-copie et le verrouillage par zone exigent des licences commerciales et ne sont pas gérés.

## Générer en ligne de commande

Le programme `bdauthor-build` génère un disque à partir d'un fichier de projet :

```bash
bdauthor-build projet.json dossier_sortie
```

Les autres outils fournis : `bdcheck` (vérifier un dossier BDMV ou une image), `bdinspect` (examiner un fichier BDMV), `bdiso` (créer une image ISO).
