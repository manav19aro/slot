# slot — RetroAchievements fork

The main purpose of this fork is to add **RetroAchievements support** to [slot](https://github.com/BrandonKowalski/slot), with offline achievement tracking and optional manual syncing.

## RetroAchievements features

- Download achievement sets and badge images directly on the handheld, for one game or all compatible games.
- Earn achievements offline, with unlock banners, achievement artwork, and progress indicators where supported.
- Browse achievements from the in-game menu or by pressing **Select** on a game on the home screen.
- Choose **Local** mode for offline tracking, or **Sync Enabled** for manual uploads and downloads of account progress. Sync runs only when requested, and failed uploads are kept for retry.
- Turn achievements off entirely while preserving downloaded sets and earned progress.

Achievement support currently covers **single-player GBA games using mGBA**, in casual/softcore mode.

## Getting started

Save Wi-Fi details under **General → Wi-Fi** and sign in under **Achievements → RetroAchievements Login**. Download achievement sets from the Achievements settings or an individual game's achievement screen.

Once downloaded, achievement sets work offline. In **Sync Enabled** mode, press **Sync** to upload local unlocks and import achievements already earned on your account.

Achievement sets, badge images, progress, upload receipts, and achievement preferences live in the SD card's `Achievements/` folder. Copy the entire folder to transfer them to another device or fresh installation using the same ROMs. Login credentials and Wi-Fi settings are stored separately.

## Original project

The original slot README is preserved in [source-readme.md](source-readme.md). Refer to it for the original project's overview, supported devices, setup, and documentation links.

All credit for the original frontend goes to [BrandonKowalski/slot](https://github.com/BrandonKowalski/slot). This fork retains the project's [license](LICENSE).
