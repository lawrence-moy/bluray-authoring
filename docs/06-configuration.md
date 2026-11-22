# 6. Configuration de l'application

**Fichier › Configuration…** règle l'application elle-même. Ces réglages sont conservés sur l'ordinateur, pas dans le projet ; **À propos › À propos de bdauthor…** indique où ils sont enregistrés.

> 📷 **Capture à faire — `img/configuration.png`** : fenêtre *Configuration* entière.

| Réglage | Rôle |
|---|---|
| **Thème** | **comme le système** (par défaut), **sombre** ou **clair** ; les zones d'image (aperçus vidéo, frise des vignettes) restent sombres, comme dans un lecteur vidéo |
| **Taille de l'interface** | de 75 % à 200 % ; aussi au clavier avec **Ctrl +** et **Ctrl −** (**Ctrl 0** : 100 %), la taille choisie étant gardée |
| **Langue / Language** | langue de l'interface : français, anglais, ou une langue ajoutée par un fichier de traduction |
| **Créer une traduction…** | enregistre un fichier avec tous les textes de l'interface, à traduire ; placez le fichier traduit dans le dossier `langues` à côté du programme |
| **Ajuster le rafraîchissement de l'aperçu pendant le déplacement** | activé : l'aperçu est redessiné au rythme que la machine peut tenir (le cadre suit toujours la souris sans retard) ; la durée du dernier rendu est affichée |
| **Sauvegarde automatique du projet toutes les** *n* minutes | voir [Sauvegarde automatique](01-prise-en-main.md#sauvegarde-automatique-et-récupération) |
| **Afficher les commandes ffmpeg complètes dans le journal de génération** | pour diagnostiquer un encodage |
| **Accélération matérielle › Encodeur vidéo** | **Logiciel (x264)** par défaut, ou un encodeur de la carte graphique (Intel Quick Sync, NVIDIA NVENC, AMD AMF ; VAAPI sous Linux, Windows Media Foundation sous Windows) si la détection l'a reconnu conforme ; la vitesse par rapport à x264 est indiquée |
| **Mesurer les performances…** | encode un extrait d'une vraie vidéo avec x264 et avec l'encodeur matériel, sans rien écrire, et compare vitesse et charge du processeur (voir ci-dessous) |
| **Détecter l'accélération matérielle…** | essaie chaque encodeur matériel connu de ffmpeg avec les réglages Blu-ray, en progressif puis en entrelacé, et affiche le résultat (voir ci-dessous). Rien n'est détecté tant que vous ne cliquez pas. |
| **Chemin de ffmpeg** | dossier contenant ffmpeg et ffprobe, ou ffmpeg lui-même ; **Auto** : à côté du programme, puis le `PATH`. La ligne « Utilisé : … » confirme ce qui est trouvé. |
| **Dossier de polices supplémentaires** | les polices `.ttf` et `.otf` de ce dossier (et de ses sous-dossiers, sur un niveau) s'ajoutent à toutes les listes de polices ; le nombre de polices trouvées est affiché |
| **Dossier du cache des vignettes** | où sont gardées les vignettes des frises ; taille affichée, 300 Mo au plus (les plus anciennes sont effacées) ; **Par défaut**, **Vider le cache**, **Ouvrir le dossier** |

## L'accélération matérielle

Un encodeur matériel (la puce vidéo de la carte graphique ou du processeur) encode bien plus vite que x264, pour une qualité un peu inférieure à débit égal ; aux débits d'un Blu-ray, l'écart se voit peu.

Seuls les encodeurs de votre système sont essayés. Quick Sync, NVENC et AMF sont les plus complets ; VAAPI est la voie normale sous Linux. Media Foundation (secours sous Windows, pour une carte sans kit de son fabricant) offre peu de réglages : il n'est proposé que si son flux s'avère conforme. Sur Mac, les vidéos sont encodées par x264 : l'encodeur d'Apple ne sait pas écrire les informations de temps qu'exige le Blu-ray.

1. Dans **Fichier › Configuration…**, cliquez **Détecter l'accélération matérielle…**. L'essai dure de quelques secondes à une minute ; **Annuler** l'arrête.
2. Chaque encodeur est listé avec son résultat : ✔ *conforme Blu-ray* (avec la vitesse et la prise en charge de l'entrelacé), ou ✖ *absent de ce ffmpeg*, *non disponible sur cet ordinateur* (pas de carte ou de pilote), *a refusé les réglages Blu-ray*, *flux produit non conforme*.
3. Choisissez-le dans **Encodeur vidéo**. Le choix est gardé avec la détection ; refaites-la après un changement de pilote ou de ffmpeg.

À la génération :

- chaque vidéo encodée par le matériel est **vérifiée** ; si le flux n'est pas conforme Blu-ray, elle est réencodée par x264 (le journal le dit), et les vidéos suivantes aussi ;
- les sources **entrelacées** et les sources **1080p à 25 ou 29,97 images/s** (écrites en 1080i sur un Blu-ray) ne passent par le matériel que s'il a réussi l'essai en entrelacé ; sinon x264 ;
- le mode **remplir le disque** utilise toujours x264 (deux passes) ;
- le matériel n'a pas de réglage de qualité équivalent au CRF : la qualité choisie dans le projet est convertie en débit moyen.

### Mesurer les performances

**Mesurer les performances…** ouvre une fenêtre : choisissez une vidéo (celles du projet sont proposées, **Choisir…** en prend une autre ; sans vidéo, une **mire de test** générée pour l'occasion, en 1080p 23,976 ou en 1080p 25, cette dernière passant comme une source française par le codage 1080i), la durée de l'extrait (10 à 60 s, pris au milieu de la vidéo) et le preset x264 (celui du projet par défaut), puis **Lancer la mesure**. Rien n'est écrit sur le disque.

Trois configurations passent par la même chaîne que la génération du disque :

| Configuration | Ce qu'elle mesure |
|---|---|
| **x264 (logiciel)** | l'encodage actuel sans accélération |
| **encodeur matériel, décodage par le processeur** | l'encodage avec accélération, tel que l'appli le fait aujourd'hui |
| **encodeur matériel, décodage par la carte graphique** | la même chose, la source étant aussi décodée par la carte graphique (essai : pas encore utilisé pour la génération) |

Pour chacune : images par seconde, **vitesse** (×1 = aussi long que la vidéo), charge moyenne du **processeur** et durée. Les conclusions sous le tableau disent si l'encodeur matériel est limité par le processeur, et si décoder par la carte graphique ferait gagner du temps. Sur un portable, préférez un extrait long : le processeur ralentit quand il chauffe.

## Les polices

bdauthor n'embarque aucune police. Les menus, jaquettes et sous-titres utilisent :

- la police par défaut : celle indiquée par la variable d'environnement `BDAUTHOR_FONT`, sinon une police du système (Arial, Segoe UI ou Calibri sous Windows ; DejaVu ou Arial ailleurs) ;
- toute police installée ;
- toute police du **dossier de polices supplémentaires** ;
- tout fichier `.ttf` / `.otf` choisi par **Autre fichier…**.

Dans les listes de polices, chaque nom est écrit avec la police elle-même, suivi de son nom en police normale.

Un projet enregistre une police par son nom de fichier. Pour ouvrir le projet sur un autre ordinateur, placez les mêmes polices dans son dossier de polices supplémentaires. Une police introuvable est remplacée par la police par défaut, avec un message.

Les polices du système ont leur propre licence. Pour des disques et jaquettes destinés à être diffusés, préférez des polices libres (licence SIL OFL : Liberation, DejaVu, Noto…).
