# Refonte de la fenêtre de réglages

## Objectif

Remplacer l'interface native actuelle (`src/winui.rs`, contrôles comctl32) par une interface moderne et soignée, sans changer ce que la fenêtre fait. Elle est fonctionnelle mais austère : onglets d'aspect ancien, grand vide sous les champs, la note de page et la ligne d'état répètent la même phrase.

## Contraintes (décidées par Shin)

- **Aucun serveur local, aucun navigateur externe.** La fenêtre est créée par `lumo.exe` lui-même.
- Un seul exécutable, pas d'installeur. Taille raisonnable.
- Le thread de l'application reste unique ; ne pas déplacer le moteur d'effets.
- `src/schema.rs` reste la **source de vérité** des pages et des champs (chemins, types, bornes). Ne pas dupliquer cette liste côté interface.

## Approche retenue : WebView2 intégré

- Crate `wry` (WebView2 est préinstallé sur Windows 11 ; sur Windows 10, il peut être absent). La page HTML/CSS/JS est **embarquée dans l'exe** (`include_str!("../ui/index.html")`), sans fichier externe ni réseau.
- Fenêtre Win32 créée par `win.rs`/`winui.rs`, WebView enfant de cette fenêtre (`build_as_child`, via `raw-window-handle`). Initialiser COM en mode STA sur le thread de l'interface.
- **Repli** : si WebView2 est introuvable ou échoue à s'initialiser, ouvrir l'ancienne fenêtre native (la conserver, derrière le même point d'entrée `open`).
- Consulter la documentation de la version de `wry` choisie : l'API change d'une version à l'autre.

### Échanges Rust ↔ page

- Rust → page : au chargement, injecter le schéma (`PAGES` sérialisé en JSON) et les réglages courants. Ajouter une fonction `schema::to_json()` testée. Pour un rafraîchissement (`WM_REFRESH`), appeler `evaluate_script("lumoRefresh(<json>)")`.
- Page → Rust : `window.chrome.webview.postMessage({ path, value })` (ou l'IPC de `wry`). Rust applique via le chemin existant `apply(path, value)` → `Settings` → `with_app(|a| a.ui_apply(s))`. Actions : `clear_annotations`, `restore_cursors`, `open_json` via `ui_action`.
- Le JS ne contient aucune liste de réglages codée en dur : il génère les pages depuis le schéma.

## Direction visuelle

- Fenêtre ~760 × 560 px, barre de titre native (mode sombre via `DwmSetWindowAttribute` / `DWMWA_USE_IMMERSIVE_DARK_MODE`), thème clair/sombre suivant celui de Windows (`prefers-color-scheme`).
- Navigation par **barre latérale** (8 pages : Général, Spotlight, Anneau, Traînée, Clics, Annotations, Curseur, Raccourcis) au lieu d'onglets.
- Contrôles : interrupteurs (bool), curseurs avec valeur affichée et unité, **sélecteurs segmentés ou menus** (select), pastilles de couleur avec sélecteur (`<input type="color">` + alpha), champs texte pour les raccourcis.
- Couleur d'accent : rose de l'icône Lumo. Typographie système (Segoe UI Variable). Pas de bibliothèque JS lourde ; HTML/CSS/JS simples.
- Optionnel : un petit **aperçu en direct** de l'effet réglé.
- Une seule ligne d'état discrète (« Enregistré ✓ »). Supprimer les doublons de texte.
- Textes en français. Prévoir de quoi ajouter l'anglais plus tard (chaînes regroupées dans un objet).

## À conserver

- Application immédiate de chaque changement + enregistrement dans `settings.json`.
- Rafraîchissement quand les réglages changent ailleurs (menu de l'icône, raccourcis).
- Une seule instance de la fenêtre ; rouvrir la ramène au premier plan.
- Boutons : Effacer les annotations, Restaurer les curseurs Windows, Ouvrir settings.json.

## Réalisation (15/11/2026)

Fait, avec ces choix :

- `wry` 0.57, `WebViewBuilder::build(&parent)` plutôt que `build_as_child` : wry remplit alors la fenêtre et suit ses redimensionnements tout seul. La WebView reste enfant de la fenêtre Win32 créée par `webui.rs`.
- Fenêtre 860 × 620 (logiques), redimensionnable (minimum 720 × 520), centrée sur l'écran de la souris, cachée jusqu'au message `ready` de la page (ou 2,5 s au plus).
- Page -> Rust : `window.ipc.postMessage(JSON)`. Messages : `set {path, value}`, `action {name}`, `browse {path}`, `page {id}`, `preview {page, w, h}`, `ready`, `close`. Rust valide chaque valeur avec `schema::validate` avant `ui_apply`.
- Nouveaux types de champ dans le schéma : `Hotkey`, `Hold` (bouton « Détecter »), `File(ext)`, `Folder` (bouton « Parcourir… », `IFileOpenDialog`).
- Aperçu en direct : `src/demo.rs` fait tourner le vrai moteur sur une souris simulée ; image PNG envoyée à ~30 i/s (`lumoFrame`) seulement quand la page en a un et que la fenêtre est visible.
- Interrupteur principal (`*.enabled`) dans l'en-tête de page ; les lignes dépendantes sont grisées quand il est coupé. Raccourci déjà pris par une autre application : signalé dans la ligne d'état.
- Repli : `webui::open` renvoie `false` si WebView2 échoue, `winui` ouvre alors l'ancienne fenêtre. `lumo.exe --native-ui` force le repli.

## Vérification

- `cargo test --lib` doit rester vert (tests de schéma inclus) ; ajouter un test pour `schema::to_json()`.
- Lancer l'application et contrôler à l'œil : chaque page, un curseur de chaque type, un select, une couleur, un raccourci, le rafraîchissement depuis le menu de l'icône, le repli si WebView2 est absent.
- Demander à Shin des captures d'écran pour valider le rendu.
