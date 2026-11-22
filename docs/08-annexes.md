# 8. Annexes

## Raccourcis clavier et souris

### Partout

| Raccourci | Action |
|---|---|
| Ctrl+S | enregistrer le projet |

### Éditeur de menus et éditeur de jaquette

| Raccourci | Action |
|---|---|
| Ctrl+Z / Ctrl+Y (ou Ctrl+Maj+Z) | annuler / rétablir |
| Ctrl+C / Ctrl+X / Ctrl+V | copier / couper / coller des éléments, y compris d'une page à l'autre |
| Suppr | supprimer les éléments sélectionnés |
| clic | sélectionner |
| Maj+clic, ou rectangle de sélection (jaquette) | ajouter ou retirer un élément de la sélection |
| glisser un coin ou un côté | redimensionner ; avec Maj : proportionnel |
| flèches | déplacer : 1 px (menus) ou 0,5 mm (jaquette) ; Maj : 10 px ou 5 mm ; Ctrl : 0,1 mm (jaquette) |
| molette | zoom |
| clic molette, clic droit maintenu, Espace + glisser | déplacer la vue |
| Alt pendant un geste | désactiver la grille (jaquette) |
| Maj en déplaçant l'extrémité d'une ligne | angle par pas de 45° (jaquette) |
| double-clic sur une page ou un calque | renommer |
| double-clic sur une image de vidéo (jaquette) | choisir l'image |

### Simulateur de télécommande

| Touche | Action |
|---|---|
| flèches | bouton voisin |
| Entrée (ou clic) | valider |
| M | TOP MENU (page d'accueil) |
| Échap | quitter la simulation |

### Fenêtres de lecture (choix d'image, chapitres, sous-titres)

| Touche | Action |
|---|---|
| Espace | lecture / pause |
| ← → | une image ; avec Maj : 1 s ; avec Ctrl : 10 s (éditeur de chapitres) |
| C | ajouter un chapitre ici (éditeur de chapitres) |
| molette sur la frise | zoom ; avec Maj : défilement |

## Limites

| Élément | Maximum |
|---|---|
| vidéos par disque | 99 |
| chapitres par vidéo | 99 |
| pistes audio par vidéo | 8 |
| pistes de sous-titres par vidéo | 8 |
| vignettes par page de menu | 12 |
| poids des images de boutons d'une page | 16 Mo (mémoire décodée par le lecteur) |
| débit d'une piste Dolby Digital | 640 kb/s |
| sons de menu différents | 255 |
| délai de lancement automatique | 3600 s |

## Formats acceptés

| Type | Formats |
|---|---|
| vidéos | mp4, mkv, mov, avi, m2ts, mts, ts, webm, wmv, flv, m4v, mpg, mpeg |
| audio séparé | wav, flac, mp3, ac3, eac3, dts, aac, m4a, ogg, opus, mka… |
| sous-titres | srt, ass, ssa, vtt, smi, sami, sub, txt, sbv, ttml, dfxp, xml, lrc, stl |
| images | png, jpg, jpeg, bmp, svg, svgz |
| polices | ttf, otf |

Sur le disque : vidéo H.264, audio Dolby Digital (AC-3) ou LPCM, sous-titres PGS, menus HDMV (graphiques IG).

## Le fichier de projet

Le projet est un fichier **JSON** lisible, modifiable à la main avec précaution (clés en anglais, chemins relatifs possibles). Les champs inconnus sont conservés à l'enregistrement.

```json
{
  "title": "Mon disque",
  "language": "fra",
  "videos": [
    { "file": "film1.mp4", "title": "Film 1" }
  ],
  "encoding": { "crf": 18, "preset": "medium" }
}
```

## Glossaire

| Terme | Sens |
|---|---|
| **BDMV** | dossier contenant la structure d'un Blu-ray (index, playlists, clips, menus) |
| **HDMV** | mode des menus Blu-ray « classiques », dessinés en images (par opposition au BD-J, programmé en Java) |
| **IG** | *Interactive Graphics* : le flux qui porte les menus et leurs boutons |
| **PGS** | format des sous-titres Blu-ray, dessinés en images |
| **AC-3 / Dolby Digital** | format audio compressé courant des disques |
| **LPCM** | audio non compressé, sans perte |
| **CRF** | réglage de qualité de l'encodeur x264 : plus petit = meilleure qualité |
| **Image clé** | image d'une vidéo à partir de laquelle un lecteur peut démarrer sans décoder les précédentes |
| **TOP MENU** | touche de la télécommande qui ramène à la page d'accueil |
| **Fond perdu** | marge imprimée au-delà du format fini, rognée à la découpe |
| **UDF** | système de fichiers des disques optiques ; 2.50 pour les Blu-ray |
| **BD-R / BD-RE** | Blu-ray enregistrable une fois / réinscriptible |
