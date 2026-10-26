# Offline GBA achievement evaluation

This crate vendors a pinned MIT-licensed rcheevos runtime and wraps it in Rust.
It does not include the online client, download rules, authenticate users, or
submit awards. See native/rcheevos/UPSTREAM.md for provenance and update steps.

Create one GbaAchievements instance per loaded GBA game. Activate locally supplied
rules and call evaluate_frame after each committed emulator frame:

```rust,ignore
let mut achievements = slot_achievements::GbaAchievements::new()?;
achievements.activate(42, "0x008000=3")?;
// On the emulation thread, after core.run_frame(input):
let ids = achievements.evaluate_frame(|address, buffer| {
    core.read_memory(address, buffer)
})?;
```

Input rule addresses use RetroAchievements' GBA layout (IWRAM, EWRAM, SRAM),
not native GBA addresses. The callback receives native addresses. Reads crossing
a logical region boundary are split before translation. Unmapped/partial reads
or reader panics discard all awards for that frame and reset transient runtime
progress. Each earned ID is emitted once per instance. A rule that already holds
at activation waits until it has been false before it can trigger, following
rcheevos semantics.

The slot gameplay worker now owns the evaluator on its emulation thread.
`EmuHandle::set_gba_achievements` installs a local rule set asynchronously before
unpausing; an empty set disables it. Invalid sets leave the old evaluator intact.
`take_achievement_events` drains unlocks or a one-time evaluation-stopped error.
No rules are installed automatically yet; real game identification/set loading,
and further UI work are separate follow-ups. Callers must supply rules only
for a verified GBA/mGBA game.

Every committed single-player frame is evaluated, including fast-forward steps.
Pause, refresh, and rewind rendering do not evaluate. Link play suspends evaluation;
entering/leaving link play, rewind, and explicit state loads reset unfinished
challenge progress. Earned IDs stay earned for that evaluator instance. Installing
a replacement set starts fresh progress. There is no active run-ahead path in
this checkout. Restoring unfinished challenges across state loads will require
serializing runtime progress with emulator states in a later step.

An unavailable memory read disables the evaluator and queues one error event,
preventing repeated failures from filling the event queue. The user-facing caller
must explicitly reinstall rules after resolving the issue.

## Verification

```sh
cargo test -p slot-achievements
cargo clippy -p slot-achievements --all-targets -- -D warnings
```

The real-core test is explicitly ignored by default so an ordinary workspace test
does not require a locally built emulator. Build the pinned host core (requires
CMake and a C compiler), then run the explicit integration check:

```sh
sh cores/mgba/build.sh build e31759b24e7a4e3899285ff720d7b573ac328ae7 target/mgba vendor/mgba_libretro.dylib
cargo test -p slot-achievements --test mgba -- --ignored
```

On Linux use vendor/mgba_libretro.so as the build output. The synthetic ROM is
created by the test; no commercial ROM, BIOS, account, or internet is needed to
run it. It increments EWRAM once per VBlank; a local rule unlocks on count 3 and
must emit exactly one event across 30 frames.

Clang reports null-pointer-subtraction warnings in upstream's RC_OFFSETOF macro.
The vendored source is unmodified; these warnings are separate from Rust lint
checks. Host testing does not verify the aarch64 Linux/device build.

Gameplay integration checks (no window, account, BIOS, or ROM required):

```sh
cargo test -p slot --no-default-features --test achievement_gameplay --test emu
```

## Local packs

At launch, slot checks `Achievements/GBA/<rom-hash>.json` under the SD root,
only for a real mGBA core running single-player GBA. Pack reading/hashing happens
on the opening thread; valid rules are queued before gameplay is unpaused. Missing,
invalid, wrong-console, and mismatched packs never prevent the game from launching.
No network requests are made. Leaving the cache directory absent avoids ROM
hashing entirely when this feature is not used.

Version 1 packs contain:

```json
{
  "version": 1,
  "console_id": 5,
  "game_id": 1,
  "title": "Synthetic example only",
  "rom_hash": "900150983cd24fb0d6963f7d28e17f72",
  "achievements": [
    {
      "id": 1,
      "title": "Synthetic example",
      "description": "Not an official achievement",
      "points": 0,
      "rule": "0xH008000=3",
      "badge": null
    }
  ]
}
```

Use actual downloaded/exported definitions and the verified supported ROM hash
for real packs. A filename/content hash match only selects the pack; it does not
prove that the definitions came from RetroAchievements. Do not substitute a ROM
hash merely to bypass a compatibility requirement. Rules are parsed before use;
unsupported schemas, empty sets, duplicate IDs, corrupt JSON, and files larger
than 8 MiB are rejected. ROM hashing is restricted to uncompressed 1-byte to
32-MiB inputs using the pinned rcheevos cartridge hashing implementation.

Inspect a ROM and optionally validate its pack:

```sh
cargo run -p slot-achievements --example inspect_pack -- "sdcard/Games/Advance Wars.gba"
cargo run -p slot-achievements --example inspect_pack -- "path/to/game.gba" "path/to/pack.json"
```

The official game-data endpoints require a username and login token. Acquiring
real definitions needs an explicit authenticated preparation step or a suitable
existing cache/export. The explicit preparation command below performs this step. No credentials belong
in a local pack. Reading a prepared pack and evaluating rules requires no account
or connection.

## Explicit authenticated preparation

Run from your own Terminal. The command resolves the exact ROM hash before
prompting, asks for your RetroAchievements username/password (password hidden),
keeps the login token in process memory, and downloads official/core definitions
only. It never submits awards, starts a play session, or schedules background work.
Credentials are sent via HTTPS POST only to the official endpoint; redirects are
rejected. Raw responses and credentials are never written to disk.

```sh
python3 tools/prepare_achievements.py "sdcard/Games/Super Mario Advance 2 - Super Mario World (Europe) (En,Fr,De,Es).gba"
```

Use `--sd-root /path/to/card` to target another SD root. Rust/Cargo and built
project dependencies must be available locally: hashing and validation run with
Cargo's offline flag. The command writes to `Achievements/GBA/<hash>.json`,
validates all rules with rcheevos, and atomically replaces the previous pack only
on success. It downloads rules/metadata, not badge image files or account unlock
history. No account credentials are retained for future syncing.

The live unauthenticated lookup for the verified European ROM returned game 4969.
Authenticated download requires the user's local login and is not covered by
mock tests. Preparation helper tests:

```sh
python3 -m unittest discover -s tools/tests -v
```

Service-generated warning entries (IDs >= 101000001, matching upstream
RC_CLIENT_ACHIEVEMENT_WARNING_ID) are not earned achievements. Preparation omits
them; loading older packs removes them from counts and logs them as service
notices; the evaluator also refuses to activate them. Normal zero-point
achievements remain eligible. This prevents the synthetic "Warning: Unknown
Emulator" rule from producing an unlock banner during offline softcore play.

## Durable unlock history

Normal pack-based GBA/mGBA launches use persistent local history at
`Achievements/Progress/GBA/<rom-hash>.json` under the SD root. Histories are scoped
to console, game ID, exact ROM hash, softcore mode, and the account-neutral `local`
profile. New records include an earned timestamp and `pending_sync` state. The
format supports future `server_confirmed` records with an explicit username and
confirmation time; there is no submission or confirmation operation yet, and
pending records are not implicitly assigned to a signed-in account.

Earned IDs are restored before activating rules. Replaying, restarting, loading
an older save, or rewinding does not clear them. New awards are persisted with the
repository's atomic-write/file-sync/directory-sync mechanism before the success
notification is queued. Writes occur only on new unlocks; synchronous SD-card
writes can cause a brief pause and still need hardware testing. No initial file
is required until the first unlock. Runtime challenge counters are still reset
on save-state loads; this change persists completed achievements only.

Malformed, unreadable, or mismatched history is preserved and tracking is refused
rather than silently starting empty. Failed unlock writes stop tracking and show
an unavailable notification instead of a success banner. The failed award is
not durable; resolve the storage issue and re-earn it after restarting. Existing
saved history remains intact when atomic replacement fails. Game saves are never
modified by this mechanism. Unlocks earned before installing this build cannot be
recovered automatically.

`EmuHandle::set_gba_achievements` remains an explicit memory-only test API;
`set_persistent_gba_achievements` is used by normal pack-based startup. Storage
and cross-worker tests:

```sh
cargo test -p slot-achievements --test progress
cargo test -p slot --no-default-features --test achievement_gameplay
```

## Achievement browser

Open the pause menu and select Achievements. The offline browser shows earned
counts, titles, points, and the selected achievement's Locked/Unlocked
status. Up/Down changes selection; Left/Right jumps five entries. A opens the
full description; B returns to the list and then to the pause menu. The menu
shortcut closes the browser and resumes gameplay. Notifications wait while the
browser is open. Saved progress is re-read each time the browser opens; corrupt
history displays an error instead of misreporting every achievement as locked.
Missing packs and unsupported core/platform/link sessions have explicit empty
states. On the home shelf, Select opens the same browser for the highlighted
ROM without starting an emulator. B returns to the same shelf selection.
Home viewing reads the cached GBA pack regardless of the selected gameplay core.
The browser uses the existing menu pointer, typography, ink, and button hints.
Select modifier shortcuts retain their existing behavior.


## Cached badges and named notifications

Unlock notifications resolve the awarded ID against the current cached pack and
show the achievement name with a 64px badge in the existing haloed toast style.
The compact notification places a smaller heading and a 19–22px name beside the
badge; the name can wrap onto two lines. The native dark backing expands to cover the full
notification for contrast over gameplay.
Notifications remain queued individually and expire after four seconds. The list
and detail screens use green text for earned achievements and subdued grayscale
badges for locked achievements, with the existing arrow marking selection.

Badge PNGs live at `SDROOT/Achievements/Badges/<badge-id>.png`. Only numeric badge
IDs are accepted. Missing, corrupt, oversized, or invalid images use a small
outline placeholder and do not prevent unlocks or browsing. Decoded images are
kept at 64px for sharp banners and displayed at 32px in menus; the browser and
notification textures are reused as content changes. Image loading is cache-only, with no network access. The current Mario
pack's 57 badge files have been staged in the local SD card folder; future manual
on-device downloads can populate the same directory.


## Explicit downloads on the handheld

Open Settings → General → Wi-Fi to enter and save a network name/password.
This saves settings without connecting. Open Settings → Achievements →
RetroAchievements Login to enter a username/password and explicitly sign in over
saved Wi-Fi. The successful sign-in stores a reusable token; the password is
never saved. The keyboard uses A to type, X for letter case, Y for symbols, and
Start to advance. Visible text preserves its actual case and passwords are masked.

From the home shelf, select a GBA game, press Select, then Y (Download). When
Wi-Fi and login are configured, this goes straight to connecting and downloading.
Missing setup points to the relevant settings menu, without an inline setup
wizard. Settings → Achievements → Download All Achievements downloads compatible
GBA packs and badges across all shelves in one explicit operation. It connects
once, skips unsupported ROMs, deduplicates identical ROM hashes, and summarizes
saved packs, unsupported games, failures, duplicate ROMs, and missing badges.
Cancellation keeps completed packs and all earned progress. Network, login, or
storage errors stop the batch and report the partial result.

The worker uses BaseOS's existing wlan0 driver, wpa_supplicant, and udhcpc. Ship
`System/slotdownload.sh` and the updated launcher alongside the binary. Association
and DHCP run only for this action. The connection is torn down before the result
is shown, including failures and cancellation. B cancels; the UI waits on
Disconnecting until the worker releases Wi-Fi. A subsequent request can reuse
saved details. Link startup is held while the worker owns the radio.

`Config/wifi.json` stores Wi-Fi settings, and `Config/retroachievements.json`
stores the username and sign-in token. The earlier combined
`Config/achievements-download.json` remains readable for existing installations;
separate settings take priority. Changing Wi-Fi preserves the saved account.
FAT cards do not enforce Unix file permissions; the setup screens explain what
is saved. The TLS client has embedded public roots, verifies certificates,
rejects redirects, bounds response sizes and timeouts, and only contacts the
official API and media hosts. Raw responses and credentials are not logged.

The worker hashes the ROM locally, identifies it, filters official achievements,
validates every rule with rcheevos, downloads valid PNG badges, checks the ROM hash
again, and atomically replaces the pack. Existing local unlock history is never
modified. Failed badge requests keep a usable pack and report missing images;
Y retries and reuses valid cached badges. Gameplay loads the saved pack on its
next launch, without network access. These requests never start a service session,
submit awards, or fetch/sync account progress.

Software checks cover the controller setup flow, pack validation, preservation of
history and old packs, cancellation, missing images, credential separation, and
mocked Wi-Fi/DHCP cleanup. Live HTTPS identification succeeded for game 4969.
Association, login, and full downloads still require testing on the handheld.


## Local mode, manual account import, and moving to another device

Settings → Achievements → Achievements toggles the entire feature on or off.
Off hides the remaining achievement settings, the in-game entry and home Select
viewer, and prevents rule loading and network jobs. Packs and earned progress
remain intact. The enabled preference defaults to true for older settings and
preserves the selected Local/Sync Enabled version when toggled.

Settings → Achievements → Version selects Local (the default) or Sync Enabled.
The preference lives in `Achievements/settings.json`. Local mode hides Sync
Progress in settings and the individual game's Sync button. Both modes display
only Locked or Unlocked, without pending/upload/sync states in achievement rows.
Downloaded packs remain fully usable without credentials, Wi-Fi, or an account.
New device-earned records have a local state; older pending-sync records remain
readable and earned. Changing modes never clears or reassigns progress.

In Sync Enabled mode, Sync Progress in settings explicitly imports account-earned
IDs for downloaded GBA sets. The individual game's X: Sync action imports just
that game's progress, including from the paused in-game browser. The API calls
use `unlocks` with both softcore and hardcore modes and union their IDs, filtered
to the matching cached pack. Manual Sync also submits eligible locally earned
softcore awards with `awardachievement`; no background requests are made.
Imported IDs are merged with local records; existing local earned timestamps
are preserved. The IDs-only response has no original earned date, so newly
imported records use earned_at=0 (unknown) and keep a separate confirmation time.
In-game merges run on the emulator's owning thread, update its loaded history,
and disable imported rules without firing unlock banners. Upload receipts are
saved after each acknowledged award under `Achievements/Uploads/GBA/`. The first
explicit Sync binds each existing local award to the signed-in account; imported
awards are excluded. Receipts preserve that binding across account changes and
retries, including after game exits or shutdown. Failed requests remain queued.
RA limits backdated submissions to 14 days; local timestamps remain unchanged.
Sync results show uploads acknowledged during this attempt and newly added local
unlocks as downloaded. Repeated syncs show zero for unchanged progress. A partial
upload failure still merges downloaded IDs and successful uploads, then shows
a retry hint; unconfirmed awards remain queued for the next manual Sync.

To move to another device or fresh install, close Slot or shut down first, then
copy the entire `Achievements/` folder from the old card to the new card alongside
the same ROMs. This carries JSON packs, badge PNGs, earned progress and the mode
preference. Progress uses the ROM content hash rather than an absolute path or
device ID. The folder includes no login tokens or Wi-Fi passwords; those remain
separate in Config. A matching ROM may have a different filename or location.
Use the current Slot build on the receiving device. Merge backups deliberately
rather than overwriting a newer destination history. Local mode requires no
RetroAchievements interaction after the set has been downloaded.

Checks cover preservation of local records during import, old history support,
folder transfer to a fresh root, hidden controls in Local mode, both API modes,
unknown-ID filtering, and an imported rule followed by another locally earned
rule in the same live emulator. Account-authenticated import still needs a
handheld test with the user's sign-in.

## Measured achievement details

The paused in-game details screen displays measured counters or percentages
provided by rcheevos. Snapshots are requested from the owning emulator thread
when opening the achievement browser; no extra frames or memory reads are made.
Only locked, active achievements with a measured rule show a value. List rows
and home-screen viewing stay unchanged; no session counters are persisted or
shown as live progress when the game is closed. Resetting evaluation clears
transient measured progress, while earned unlocks remain preserved.
