# Lumo — notes pour Claude Code

Surligneur de curseur Windows en Rust (clone des effets gratuits de Mouzz) : spotlight, anneau (pulse, RGB), 7 traînées, 7 clics, sons, shake-to-locate, annotations (touche maintenue + souris), curseur custom osu!, raccourcis globaux, icône de zone de notification, fenêtre de réglages native.

Propriétaire : Shin (français, dev solo). Répondre en français, réponses directes, fichiers complets (pas de diffs partiels), récaps en `.md`. Il préfère `nano` à `sed` : donner des commandes à copier-coller.

## Build et tests

```powershell
cargo test --lib            # moteur + schéma (portable, tourne aussi sous Linux/WSL)
cargo run --release         # lance Lumo (Windows, toolchain MSVC)
cargo run --bin preview     # planches PNG de contrôle du rendu (docs/)
cargo run --bin defaults > settings.example.json
```

`lumo.exe --restore-cursors` rend les curseurs Windows si le programme a été tué brutalement. `--minimized` démarre sans ouvrir la fenêtre.

## Architecture

- `src/settings.rs` : réglages serde, couleurs `#RRGGBB[AA]`, `sanitize()`, parsing des raccourcis (`parse_hotkey`) et des touches maintenues (`parse_hold`).
- `src/fx.rs` : moteur d'effets, rendu `tiny-skia` (portable, testé). Aucune dépendance Win32.
- `src/skin.rs` : skins osu! (`cursor`, `cursormiddle`, `cursortrail`, `@2x` préféré).
- `src/schema.rs` : description des réglages (pages, champs, bornes) qui pilote la fenêtre. Les tests vérifient que chaque chemin existe, que les types collent et que chaque réglage a un champ.
- `src/win.rs` : couche Win32 (overlays par écran, hook souris `WH_MOUSE_LL`, tray, hotkeys, curseurs système).
- `src/winui.rs` : fenêtre de réglages native (comctl32 v6), générée depuis `schema.rs`.
- `build.rs` + `lumo.manifest` : manifeste comctl32 v6 + DPI par écran (MSVC uniquement).

Règles à garder :

- Tout tourne sur un seul thread. L'état est dans des `thread_local!` accédés avec `try_borrow_mut` (`with_app`, `with_ui`). Relâcher les emprunts avant tout appel modal (`TrackPopupMenu`, `ChooseColorW`).
- Ajouter un réglage = `settings.rs` + une entrée dans `schema.rs` (sinon un test échoue) + effet dans `fx.rs` si besoin.
- Les noms d'enums serde sont stables (`circle|h_bar|v_bar`, `line|comet|glow|dots|stars|hearts|rainbow|skin`, `ring|ripple|pulse|burst|confetti|stars|hearts`, `off|system|overlay`).

## État

- Écrit sans pouvoir compiler pour Windows (environnement Linux) : la couche Win32 a seulement passé un type-check. Sur Windows, elle compile et tourne (la fenêtre de réglages s'affiche, voir captures de Shin).
- Pas de test automatisé de `win.rs` / `winui.rs` : vérifier à la main.

## À faire (par ordre voulu par Shin, « petit à petit »)

1. **Interface de réglages plus jolie.** La fenêtre native fonctionne mais est austère : grand vide sous les champs, la note de page et la ligne d'état répètent la même phrase, onglets d'aspect ancien. Pistes : WebView2 intégré (crate `wry`, HTML embarqué dans l'exe, communication par IPC, aucun serveur local : Shin ne veut pas de serveur ni de navigateur externe) ou rendu custom avec `tiny-skia`. Garder `schema.rs` comme source des champs.
2. **Latence de la traînée** (constatée par Shin). Pistes : ne repeindre que la zone modifiée, ne rendre que l'écran du curseur, utiliser les positions du hook souris au lieu de `GetCursorPos` toutes les 8 ms, limiter les pixmaps temporaires (anneau RGB).
3. **Touche d'annotation.** Fn est invisible pour Windows. Shin a un Corsair K55 : il remappe une touche vers F13 dans iCUE, puis `annotate.key = "F13"`. Ajouter un bouton « Détecter la touche » dans la fenêtre de réglages.
4. Plus tard : portage Linux (Fedora KDE). Le moteur `fx.rs` est portable ; seule la couche `win.rs`/`winui.rs` est à refaire.
