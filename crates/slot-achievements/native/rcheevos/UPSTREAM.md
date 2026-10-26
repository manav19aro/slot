Source: https://github.com/RetroAchievements/rcheevos
Commit: bbc79b58a5a2a0b98dfbf2be6ca9e42c764661ce
License: MIT (see LICENSE).

Unmodified include/, src/runtime/, and src/util/ are vendored here. Only the
runtime, utility, and minimal cartridge hashing C sources are compiled; no client, HTTP, or account code is
included. src/hash/rc_hash.c and rc_hash_internal.h were also copied from
the same commit; optional disc/ROM-format/encrypted/ZIP modules are disabled
(GBA uses whole-buffer hashing). native/bridge.c is slot's wrapper, outside this upstream directory.

To update, replace these directories from an explicit upstream commit, update
this record, and rerun slot-achievements tests including the mGBA integration test.
