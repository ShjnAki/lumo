# Lumo — surligneur de curseur pour Windows

Clone libre, inspiré de [Mouzz](https://mouzz.dev/) (macOS), écrit en Rust. Il ajoute le **curseur custom façon osu!** que tu voulais.

![Spotlight, ring et curseur](docs/1_spot_ring_cursor.png)

## Fonctionnalités

| Fonction (équivalent Mouzz gratuit) | Statut |
|---|---|
| Spotlight : cercle, barre horizontale, barre verticale ; taille, assombrissement, douceur | ✅ |
| Ring : taille, épaisseur, couleur, pulsation | ✅ (+ halo « neon », qui est du Pro chez Mouzz) |
| Traînée : 7 styles (ligne, comète, lueur, points, étoiles, cœurs kawaii, arc-en-ciel) | ✅ (+ un 8ᵉ style : `cursortrail.png` d'un skin osu!) |
| Effets de clic : 7 styles (anneau, onde, pulsation, rayons, confettis, étoiles, cœurs) ; couleur distincte clic droit | ✅ |
| Sons de clic : son intégré ou fichier `.wav` | ✅ |
| Secouer la souris pour localiser le curseur | ✅ |
| Raccourcis globaux personnalisables (aucun par défaut) | ✅ |
| Icône de zone de notification : activer/désactiver, choix des styles | ✅ |
| **Curseur custom osu!** (mode système ou overlay, skins `cursor.png`/`cursormiddle.png`/`cursortrail.png`) | ✅ (ajout) |
| Annotations à l'écran (Fn) | ❌ la touche Fn est invisible pour Windows |
| Pro : enregistreur, sous-titres, caméra, frappes, zoom, loupe, focus mode | ❌ hors périmètre |

Les réglages se font par l'icône de la zone de notification (clic droit) **ou** dans `%APPDATA%\Lumo\settings.json`. Le fichier est rechargé à chaud, sans redémarrage. Il n'y a pas de fenêtre de réglages graphique pour l'instant.

## Compiler

Prérequis : Rust (`rustup`) avec la toolchain MSVC (Visual Studio Build Tools, charge de travail « Desktop development with C++ »). Tu as déjà ça pour le projet Tauri.

```powershell
cargo build --release
# -> target\release\lumo.exe  (exécutable autonome)
```

Sans toolchain locale, pousse le dépôt sur GitHub : le workflow `.github/workflows/build.yml` compile sur `windows-latest` et publie `lumo.exe` en artefact.

## Utilisation

1. Lance `lumo.exe` : une icône rose apparaît près de l'horloge. Le ring et les clics sont actifs par défaut.
2. Clic gauche sur l'icône : active/désactive tout. Clic droit : menu complet.
3. `lumo.exe --restore-cursors` rend les curseurs Windows d'origine si le programme a été tué brutalement (gestionnaire des tâches).

### Curseur osu!

Dans `settings.json` :

```json
"cursor": {
  "mode": "overlay",
  "size": 48,
  "color": "#ff66aa",
  "skin_dir": "C:/Users/toi/AppData/Local/osu!/Skins/MonSkin",
  "press_scale": 1.25,
  "rotate": false
}
```

- `"mode": "system"` : le vrai curseur Windows est remplacé par l'image (zéro latence, pas d'animation).
- `"mode": "overlay"` : le curseur natif est masqué et l'app dessine le sien, avec agrandissement au clic (`press_scale`) et rotation optionnelle (`rotate`).
- `skin_dir` : dossier d'un skin osu! (variantes `@2x` préférées). Laisse vide pour le curseur généré. Utilise des `/` ou des `\\` dans le JSON.
- Pour utiliser la traînée du skin : `"trail": { "enabled": true, "style": "skin" }`.

### Raccourcis

```json
"hotkeys": { "toggle_all": "Ctrl+Alt+L", "toggle_spotlight": "Ctrl+Alt+S" }
```

Un modificateur (`Ctrl`, `Alt`, `Shift`, `Win`) est obligatoire. Touches : lettres, chiffres, `F1`–`F24`. Les raccourcis déjà pris par une autre application sont ignorés sans message.

Toutes les clés sont listées dans `settings.example.json`. Les couleurs sont en `#RRGGBB` ou `#RRGGBBAA`.

## Architecture

```
src/settings.rs   réglages JSON (serde), couleurs, parsing des raccourcis
src/fx.rs         moteur : état + rendu tiny-skia (portable, testé)
src/skin.rs       chargement des skins osu! (PNG)
src/win.rs        couche Win32 : overlays, hook souris, tray, hotkeys, curseurs
src/bin/preview   rendu de contrôle en PNG (planches dans docs/), tourne sous Linux
```

- Une fenêtre `WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_NOACTIVATE` par écran, alpha par pixel via `UpdateLayeredWindow`. Elle ne capture aucun clic.
- Les clics viennent d'un hook `WH_MOUSE_LL` ; la position vient de `GetCursorPos`, relevée toutes les ~8 ms.
- Un écran sans rien à dessiner voit sa fenêtre masquée. Au repos, le rendu s'arrête.

## État de la vérification

- Moteur d'effets : 7 tests unitaires, rendus PNG relus (`docs/`).
- Couche Win32 : **type-check complet contre `windows-sys 0.59` réussi**, mais ni liée ni exécutée dans l'environnement de création (pas de cible Windows). Premier lancement : à surveiller.

Points à vérifier en premier si quelque chose cloche :

1. **Halo autour du curseur système** : l'alpha de `CreateIconIndirect` est fourni prémultiplié. Si les bords semblent sales, passe en `overlay`, ou dé-prémultiplie dans `make_icon` (`win.rs`).
2. **Jeux en plein écran exclusif** : aucun overlay ne passe au-dessus (limite de Windows). Le plein écran « sans bordure » fonctionne.
3. **Performance sur très grand écran** : chaque écran visible est repeint à chaque image. Piste d'optimisation : ne repeindre que la zone modifiée.
4. **Curseurs applicatifs custom** (certains jeux, navigateurs avec `cursor: url()`) : non remplacés, car seuls les curseurs système standards le sont.
