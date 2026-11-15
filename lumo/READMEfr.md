# Lumo — surligneur de curseur pour Windows

[🇬🇧 English](README.md) · 🇫🇷 Français

Lumo est un surligneur de curseur libre et open source pour Windows, écrit en Rust. Il s'inspire de [Mouzz](https://mouzz.dev/) (macOS) dont il reprend les effets gratuits, puis y ajoute un **curseur personnalisé façon osu!**, un **anneau arc-en-ciel (RGB)** et des **annotations à l'écran**. Lumo est un projet indépendant, sans lien avec Mouzz.

![Spotlight, anneau et curseur](docs/1_spot_ring_cursor.png)

## Fonctionnalités

| Fonction | Statut |
|---|---|
| Spotlight : cercle, barre horizontale, barre verticale ; taille, assombrissement, douceur | ✅ |
| Anneau : taille, épaisseur, couleur, pulsation, halo néon | ✅ (+ effet **arc-en-ciel / RGB** rotatif) |
| Traînée : 7 styles (ligne, comète, lueur, points, étoiles, cœurs, arc-en-ciel) | ✅ (+ un 8ᵉ style utilisant le `cursortrail.png` d'un skin osu!) |
| Effets de clic : 7 styles (anneau, onde, pulsation, rayons, confettis, étoiles, cœurs) ; couleur distincte pour le clic droit | ✅ |
| Sons de clic : son intégré ou fichier `.wav` personnel | ✅ |
| Secouer la souris pour localiser le curseur | ✅ |
| Raccourcis globaux personnalisables (aucun par défaut) | ✅ |
| Icône de la zone de notification : activer/désactiver les effets, choisir les styles | ✅ |
| Fenêtre de réglages avec aperçu en direct | ✅ |
| **Curseur personnalisé façon osu!** (mode système ou overlay, skins osu! pris en charge) | ✅ |
| Annotations à l'écran : maintenir une touche et déplacer la souris pour dessiner | ✅ (touche configurable, `Ctrl+Alt` par défaut) |
| Fonctions Pro de Mouzz (enregistreur, sous-titres, caméra, frappes, zoom, loupe, mode focus) | ❌ hors périmètre |

## Installation

Prérequis : Windows 10 (version 1703 ou ultérieure) ou Windows 11.

**Depuis les sources**, avec [Rust](https://rustup.rs/) et la toolchain MSVC (Visual Studio Build Tools, charge de travail « Développement Desktop en C++ ») :

```powershell
git clone https://github.com/ShjnAki/lumo.git
cd lumo
cargo build --release
# -> target\release\lumo.exe  (exécutable autonome)
```

Le workflow GitHub Actions `.github/workflows/build.yml` compile aussi `lumo.exe` sur `windows-latest` et le publie comme artefact de build.

## Utilisation

1. Lancer `lumo.exe`. Une icône apparaît près de l'horloge et la fenêtre de réglages s'ouvre. L'anneau et les effets de clic sont actifs par défaut.
2. Un **clic gauche** sur l'icône rouvre la fenêtre de réglages ; un **clic droit** affiche le menu complet. Avec `lumo.exe --minimized`, Lumo démarre directement dans la zone de notification.
3. Chaque modification est appliquée immédiatement et enregistrée dans `%APPDATA%\Lumo\settings.json`. Le fichier est aussi rechargé à chaud lorsqu'il est modifié à la main.
4. Si Lumo est arrêté brutalement (gestionnaire des tâches), `lumo.exe --restore-cursors` rétablit les curseurs Windows d'origine.

Tous les réglages sont listés dans [`settings.example.json`](settings.example.json). Les couleurs s'écrivent `#RRGGBB` ou `#RRGGBBAA`.

### Curseur façon osu!

```json
"cursor": {
  "mode": "overlay",
  "size": 48,
  "color": "#ff66aa",
  "skin_dir": "C:/Users/vous/AppData/Local/osu!/Skins/MonSkin",
  "press_scale": 1.25,
  "rotate": false,
  "outline": false
}
```

- `"mode": "system"` : le vrai curseur Windows est remplacé par l'image (aucune latence ajoutée, pas d'animation).
- `"mode": "overlay"` : le curseur natif est masqué et Lumo dessine le sien, avec un agrandissement au clic (`press_scale`) et une rotation optionnelle (`rotate`).
- `skin_dir` : dossier d'un skin osu! (`cursor`, `cursormiddle`, `cursortrail` ; variantes `@2x` privilégiées). Laisser vide pour le curseur généré. Dans le JSON, les chemins s'écrivent avec `/` ou `\\`.
- `outline` (désactivé par défaut) : ajoute un liseré blanc autour du curseur généré.
- Pour utiliser la traînée du skin : `"trail": { "enabled": true, "style": "skin" }`.

### Anneau RGB

```json
"ring": { "rgb": true, "rgb_speed": 120, "thickness": 6, "glow": true }
```

Un dégradé arc-en-ciel tourne autour de l'anneau. `rgb_speed` est en degrés par seconde (`360` = un tour par seconde, valeur négative = sens inverse). `rgb` remplace la teinte de `color` mais conserve son opacité. L'effet s'applique aussi à l'anneau « secouer pour localiser » et s'active depuis le menu de l'icône : *Anneau → Effet RGB*.

### Annotations

Maintenir **Ctrl gauche + Alt gauche** et déplacer la souris : un tracé se dessine, sans clic. Une fois les touches relâchées, il s'efface au bout de 2,5 s.

![Annotations](docs/4_annotations.png)

```json
"annotate": { "enabled": true, "key": "Ctrl+Alt", "color": "#ff3b30", "width": 5, "fade_delay": 2.5 }
```

- `key` : touches maintenues ensemble (`Ctrl`, `Alt`, `Shift`, `Win` = touches de gauche ; `RCtrl`, `RAlt`, `RShift`, `RWin`, `CapsLock`, lettres, chiffres, `F1`–`F24`). Les touches de gauche évitent AltGr, qui équivaut à Ctrl gauche + Alt droit sur de nombreuses dispositions (AZERTY, par exemple).
- `fade_delay` : secondes avant l'effacement ; `0` conserve le tracé jusqu'à `clear_annotations` (raccourci ou menu de l'icône).
- **La touche `Fn` est inutilisable.** Sur presque tous les claviers, elle est gérée par le firmware du clavier et Windows ne la voit jamais. Une solution consiste à réassigner une touche vers `F13`–`F24` avec le logiciel du clavier (iCUE, QMK/VIA, outil du constructeur), puis à régler `"key": "F13"`.

### Raccourcis

```json
"hotkeys": { "toggle_all": "Ctrl+Alt+L", "toggle_spotlight": "Ctrl+Alt+S", "clear_annotations": "Ctrl+Alt+C" }
```

Au moins un modificateur (`Ctrl`, `Alt`, `Shift`, `Win`) est obligatoire. Touches admises : lettres, chiffres, `F1`–`F24`. Un raccourci déjà utilisé par une autre application est ignoré sans message.

## Limites connues

- **Latence de la traînée** : la traînée accuse un retard visible sur le curseur. Chaque écran visible est actuellement repeint en entier à chaque image, et la position est relevée toutes les ~8 ms. Corrections prévues : ne repeindre que la zone modifiée, ne rendre que l'écran actif, utiliser les positions du hook souris. Voir la feuille de route ci-dessous.
- **Jeux en plein écran exclusif** : aucun overlay ne peut s'afficher par-dessus (limite de Windows). Le plein écran sans bordure fonctionne.
- **Curseurs définis par les applications** (certains jeux, pages web utilisant `cursor: url()`) : non remplacés, car seuls les curseurs système standards le sont.
- **Mode curseur système** : le canal alpha transmis à `CreateIconIndirect` est prémultiplié. Si les bords paraissent sales sur une configuration donnée, passer en mode `overlay`.

## Feuille de route

- Une interface de réglages plus soignée.
- Optimisation de la latence de la traînée.
- Un bouton « Détecter la touche » dans la fenêtre de réglages, pour choisir la touche d'annotation.
- Un portage Linux (le moteur d'effets est portable ; seule la couche de fenêtrage est propre à Windows).

## Architecture

```
src/settings.rs   réglages JSON (serde), couleurs, analyse des raccourcis
src/fx.rs         moteur d'effets : état + rendu tiny-skia (portable, testé)
src/skin.rs       chargement des skins osu! (PNG)
src/schema.rs     description des réglages (pages, champs, bornes) qui pilote la fenêtre ; testée
src/win.rs        couche Win32 : overlays, hook souris, icône de notification, raccourcis, curseurs
src/winui.rs      fenêtre de réglages native (onglets, curseurs, sélecteur de couleur), sans serveur ni navigateur
build.rs + lumo.manifest   styles de contrôles Windows modernes (comctl32 v6) et DPI par écran (MSVC)
src/bin/preview   génère des planches PNG de contrôle (enregistrées dans docs/), fonctionne sous Linux
```

- Une fenêtre `WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_NOACTIVATE` par écran, avec alpha par pixel via `UpdateLayeredWindow`. Elle ne capture jamais un clic.
- Les clics proviennent d'un hook `WH_MOUSE_LL` ; la position provient de `GetCursorPos`, relevée toutes les ~8 ms.
- Un écran sans rien à dessiner voit sa fenêtre masquée. Au repos, le rendu s'arrête.
- Tout s'exécute sur un seul thread ; l'état de l'application réside dans des cellules `thread_local!`.

## Développement et tests

```powershell
cargo test --lib            # moteur d'effets et schéma des réglages (portable)
cargo run --release         # lancer Lumo (Windows, toolchain MSVC)
cargo run --bin preview     # générer les planches PNG de contrôle des effets (docs/)
```

- Le moteur d'effets et le schéma des réglages sont couverts par 18 tests unitaires ; le rendu est contrôlé grâce aux planches PNG de `docs/`.
- La couche Win32 (overlays, hook, icône de notification, fenêtre de réglages) n'a pas de tests automatisés et est vérifiée à la main sous Windows.

## Contribuer

Les issues et pull requests sont les bienvenues. Pour ajouter un réglage, il faut mettre à jour `src/settings.rs` et `src/schema.rs` : les tests du schéma échouent si un réglage n'a pas de champ correspondant dans la fenêtre.
