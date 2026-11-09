<div align="center">

<img src="logo.png" alt="Logo de e" width="128">

# e

</div>

<div align="center">

[English](../README.md) | [简体中文](README.zh-CN.md) | [日本語](README.ja-JP.md) | Français

</div>

`e` est une interface de terminal pour les agents de programmation : `dshe` se connecte à [DeepSeek Harness (DSH)](https://github.com/deepseek-ai/deepseek-harness) et `pie` exécute Pi via son interface RPC officielle, sans remplacer leur environnement d’exécution.

> Le projet est encore à un stade précoce. Windows et PowerShell constituent l’environnement principalement pris en charge ; des binaires précompilés sont aussi disponibles pour Linux x64.

https://github.com/user-attachments/assets/45a75c55-7e3b-4af3-b1e2-1c6306b80358

## Démarrage rapide

### Installation

Choisissez l’interface adaptée à votre agent. Les commandes ci-dessous nécessitent [Node.js et npm](https://nodejs.org/), ainsi que [Rust et Cargo](https://rustup.rs/). DSH nécessite également [Git](https://git-scm.com/) et pnpm.

**DeepSeek Harness — `dshe`**

```bash
npm install --global pnpm @deepseek-ai/dsh
cargo install e-dsh --locked
dshe setup
```

**Pi — `pie`**

```bash
npm install --global @earendil-works/pi-coding-agent
cargo install e-pi --locked
```

Vous préférez éviter l’installation de Rust ? Téléchargez l’archive ZIP Windows x64 ou tarball Linux x64 depuis [GitHub Releases](https://github.com/gloridifice/e/releases), vérifiez-la avec `SHA256SUMS`, puis extrayez les exécutables dans un répertoire du `PATH`. Chaque archive contient les deux interfaces. Installez les dépendances npm de l’agent choisi avec les commandes ci-dessus ; DSH nécessite toujours Git et l’exécution de `dshe setup`.

Lancez `dshe` ou `pie` dans le répertoire de votre projet, puis utilisez `/login` pour configurer l’accès à votre fournisseur. `dshe` démarre automatiquement son service DSH dédié ; `pie` réutilise les identifiants, modèles, extensions et fichiers de session natifs de Pi.

Utilisez `/help` dans l’une ou l’autre interface pour consulter les commandes au clavier, et `pie --help` pour les options de lancement de Pi. Consultez le [guide d’installation et de mise à jour de DSH](../doco/guides/dsh-integration.md) et les [prérequis du runtime Pi](../doco/guides/development.md#prerequisites) pour les détails de compatibilité.

### Mise à jour

Relancez les commandes d’installation npm et Cargo correspondant à l’interface choisie. Si vous utilisez les binaires précompilés, remplacez les exécutables par ceux de la dernière version.

Après une mise à jour de DSH ou de `dshe`, relancez `dshe setup`, puis redémarrez tout service DSH actif pour charger le nouveau bridge. `/reload` ne remplace pas ce redémarrage.

## Fonctionnalités

- Affichage progressif du Markdown avec coloration syntaxique, tableaux, diagrammes Mermaid et aperçu des diffs.
- Panneau Preview redimensionnable et mode Reading pour consulter les messages et les sorties des outils.
- Édition multiligne, historique de saisie, complétion des commandes et des chemins, et mise en file des prompts.
- Sélection du modèle et de l’effort de raisonnement, macros de prompts réutilisables et choix du modèle de compactage du contexte.
- Authentification native de Pi, gestion MCP, reprise, bifurcation et duplication des sessions.
- Commandes shell locales exclues du contexte du modèle et de l’historique de l’agent ; elles sont non interactives et s’exécutent avec vos permissions.
- Historique d’exécution avec durées des opérations et chronologie des tokens et des coûts. Les métadonnées exportées peuvent contenir des arguments de commande sensibles.
- Thèmes personnalisés, raccourcis configurables et interface en anglais ou en chinois simplifié.

## Configuration

Utilisez `/settings` pour les préférences courantes ou `/econfig` pour localiser la configuration commune aux deux interfaces :

- Windows : `%APPDATA%\e`
- Linux : `$XDG_CONFIG_HOME/e` (par défaut `~/.config/e`)
- macOS : `~/.config/e`

Les paramètres sont stockés dans `config.toml` ; définissez `language = "zh-CN"` pour activer le chinois simplifié. L’anglais est la langue par défaut et de repli. Les identifiants et fichiers de session des agents restent gérés par DSH ou Pi.

Personnalisez `key_mapping.toml` à l’aide du [guide des raccourcis](../doco/guides/key-mapping.md) et des [raccourcis par défaut](../crates/e-tui/assets/default_key_mapping.toml). Ajoutez vos fichiers TOML de thèmes dans `themes/`, en prenant les [thèmes fournis](../crates/e-tui/assets/themes/) comme exemples. `/reload` recharge la configuration, les thèmes et les ressources du backend.

## Compiler depuis les sources

Git et Rust/Cargo sont nécessaires. Les prérequis d’exécution indiqués plus haut restent applicables.

```bash
git clone https://github.com/gloridifice/e.git
cd e
cargo build --release --locked -p e-dsh -p e-pi --bins
```

Les exécutables se trouvent dans `target/release/`. Pour l’installation depuis une copie du dépôt, les modifications du bridge et les builds de profilage, consultez le [guide de développement](../doco/guides/development.md).

## Licence

Sous licence [MIT](../LICENSE-MIT) ou [Apache-2.0](../LICENSE-APACHE), au choix.

## Développement

Les issues sont les bienvenues. Les pull requests ne sont pas encore acceptées tant que le projet reste un prototype.

- [Index de la documentation](../doco/README.md)
- [Guide de développement](../doco/guides/development.md)
- [Architecture](../doco/architecture.md)
- [Tests et validation](../doco/guides/testing.md)
- [Dépannage](../doco/guides/troubleshooting.md)
