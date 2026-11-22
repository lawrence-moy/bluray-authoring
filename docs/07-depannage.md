# 7. Dépannage et questions fréquentes

## Installation et démarrage

**« ffmpeg introuvable » ou les vidéos restent « (inconnue) ».**
bdauthor ne trouve pas ffmpeg ou ffprobe. Copiez `ffmpeg.exe` et `ffprobe.exe` à côté de `bdauthor.exe`, ou indiquez leur dossier dans **Fichier › Configuration…** : la ligne « Utilisé : … » doit apparaître. ffmpeg doit inclure **libx264** et l'encodeur **ac3** (les versions « essentials » et « full » pour Windows conviennent).

**La fenêtre d'écoute d'une piste audio ne joue rien, ou affiche « la sortie audio ne répond pas ».**
Elle joue le son par la sortie audio par défaut du système, comme les aperçus vidéo. Vérifiez cette sortie (pilote, casque ou enceintes débranchés), puis relancez la lecture.

**Les fenêtres vidéo sont muettes, avec « sans son : … ».**
Aucune sortie audio n'a été trouvée, ou elle ne répond pas (pilote, périphérique débranché). Vérifiez la sortie audio par défaut du système. La lecture des images continue en muet. Vérifiez aussi que le menu 🔊 n'est pas sur **Muet** et que la vidéo a bien une piste audio.

## Vidéos, audio et sous-titres

**Le son est décalé par rapport à l'image sur le disque.**
Réglez le **Décalage +/- (ms)** de la piste (✏) : positif pour retarder le son, négatif pour l'avancer. La fenêtre d'écoute (🔊) et les fenêtres de lecture aident à trouver la bonne valeur.

**La génération refuse la « Copie de la piste AC-3 de la source ».**
La piste source doit être du Dolby Digital à 48 kHz, avec 6 canaux et 640 kb/s au plus. Le message indique ce qui ne va pas. Choisissez alors **Dolby Digital 2.0** ou **5.1** pour réencoder la piste.

**Mon lecteur ne lit pas une piste LPCM.**
Tous les lecteurs n'acceptent pas toutes les variantes du LPCM. Essayez le LPCM 16 bits, ou revenez au Dolby Digital. Le LPCM de bdauthor n'est pas encore validé sur tous les lecteurs de salon.

**Des caractères des sous-titres sont faux (é, ç, lettres cyrilliques…).**
L'encodage du fichier a été mal deviné. Dans les réglages de la piste (✏), choisissez l'**Encodage du fichier** à la main : Windows-1252 pour le français ancien, Windows-1251 pour le russe, etc.

**Les sous-titres d'un fichier MicroDVD (.sub) sont décalés progressivement.**
Ces fichiers comptent en images : indiquez la **Cadence** de la vidéo d'origine (23,976, 25…).

**Les sous-titres intégrés à ma vidéo ne sont pas proposés.**
Seuls les sous-titres texte sont gérés. Les sous-titres en images (DVD, PGS) ne le sont pas.

## Menus

**Le bouton « sous-titres en anglais » affiche une autre langue dans certaines vidéos.**
Pour les sous-titres, le rang de la langue dans la liste du projet fixe le numéro de piste, identique dans toutes les vidéos. Une vidéo qui n'a pas cette langue n'affiche rien ou autre chose. **Vérifier les menus** signale ces cas (règle *Langues*).

**Une page n'apparaît jamais sur le disque.**
Aucun bouton n'y mène depuis la page d'accueil. **Vérifier les menus** la signale (règle *Accessibilité*). Ajoutez un bouton vers elle, ou définissez-la comme page d'accueil.

**On ne peut pas atteindre un bouton avec la télécommande.**
Avec des flèches réglées à la main, un bouton peut devenir injoignable. **Vérifier les menus** le signale (règle *Navigation aux flèches*). Testez avec **▶ Simuler la télécommande**.

**Le menu ne s'affiche pas sur certains lecteurs, ou met longtemps à apparaître.**
La page est trop lourde : regardez le **Poids des boutons** dans l'inspecteur. Allégez les halos, les agrandissements, les animations, les images dessinées, ou réduisez le nombre de boutons.

**Un texte est caché par un bouton.**
L'image d'un bouton est dessinée par-dessus les textes non cliquables. L'éditeur le signale par un clignotement orange : déplacez le texte ou le bouton.

## Génération du disque

**« Corrigez d'abord : … » et rien ne se passe.**
Le projet a des erreurs bloquantes : fichier introuvable, destination de bouton manquante, langue à choisir… Corrigez chaque point listé.

**Le contenu ne tient pas sur le disque.**
La jauge d'occupation (en haut à droite) le signale dès l'édition, en rouge. Baissez la qualité (CRF plus élevé), activez **Viser un débit qui remplit le disque**, passez à un disque plus grand (BD-50), réduisez le nombre de pistes audio LPCM, ou retirez des vidéos.

**La génération est très longue.**
L'encodage vidéo est l'étape la plus longue. Un préréglage plus rapide accélère au prix d'un fichier plus gros. **Copier les vidéos déjà conformes** évite tout réencodage pour les vidéos H.264 compatibles, et le **cache** évite de réencoder ce qui n'a pas changé.

**La vérification affiche des erreurs.**
Gardez le journal de génération : il aide à diagnostiquer. Signalez le problème avec le journal (et, si possible, le fichier de projet).

## Jaquette

**La jaquette imprimée n'a pas la bonne taille.**
Imprimez le PDF **à 100 %**, sans « ajuster à la page » ni marges automatiques de l'imprimante. Vérifiez la taille avec les repères de coupe.

**Un élément de la jaquette n'est pas exporté.**
Il est peut-être sur un calque **masqué** : un calque masqué n'est ni affiché ni exporté.

**Le code-barres ne se lit pas.**
Gardez un fond clair (blanc par défaut) avec les zones de silence, une taille suffisante et un bon contraste.
