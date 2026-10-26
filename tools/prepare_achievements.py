#!/usr/bin/env python3
"""Explicit, one-shot GBA achievement preparation; no background networking."""
import argparse
import getpass
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import urllib.error
import urllib.parse
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
ENDPOINT = "https://retroachievements.org/dorequest.php"
MAX_RESPONSE = 8 * 1024 * 1024
# Matches RC_CLIENT_ACHIEVEMENT_WARNING_ID in pinned upstream rc_client_game.h.
SERVICE_WARNING_ID = 101000001


class PreparationError(Exception):
    pass


class NoRedirect(urllib.request.HTTPRedirectHandler):
    # Never forward an authentication request to another URL.
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None


def request(fields):
    data = urllib.parse.urlencode(fields).encode("utf-8")
    req = urllib.request.Request(ENDPOINT, data=data, headers={
        "Content-Type": "application/x-www-form-urlencoded",
        "User-Agent": "slot-local-pack-preparer/1.0",
    })
    try:
        with urllib.request.build_opener(NoRedirect()).open(req, timeout=30) as response:
            body = response.read(MAX_RESPONSE + 1)
        if len(body) > MAX_RESPONSE:
            raise PreparationError("Server response exceeded 8 MiB.")
        result = json.loads(body)
    except (urllib.error.URLError, TimeoutError, OSError, ValueError) as error:
        # Do not echo request data, server response bodies, or credentials.
        raise PreparationError("RetroAchievements request failed. Check connectivity and try again.") from None
    if not isinstance(result, dict) or result.get("Success") is not True:
        raise PreparationError("RetroAchievements rejected the request. Check your login or game compatibility.")
    return result


def cargo_environment():
    env = os.environ.copy()
    cargo = shutil.which("cargo")
    if not cargo:
        for candidate in [Path.home() / ".cargo/bin/cargo", Path("/opt/homebrew/opt/rustup/bin/cargo"), Path("/usr/local/opt/rustup/bin/cargo")]:
            if candidate.is_file():
                cargo = str(candidate)
                break
    if not cargo:
        raise PreparationError("Rust/Cargo is required to identify the ROM and validate rules.")
    env["PATH"] = str(Path(cargo).parent) + os.pathsep + env.get("PATH", "")
    return cargo, env


def inspect(rom, pack=None):
    cargo, env = cargo_environment()
    command = [cargo, "run", "--offline", "--quiet", "-p", "slot-achievements", "--example", "inspect_pack", "--", str(rom)]
    if pack:
        command.append(str(pack))
    result = subprocess.run(command, cwd=ROOT, env=env, capture_output=True, text=True)
    if result.returncode:
        raise PreparationError("ROM/pack validation failed. Run the inspect_pack example directly for diagnostics.")
    match = re.search(r"^GBA RetroAchievements hash: ([0-9a-f]{32})$", result.stdout, re.MULTILINE)
    if not match:
        raise PreparationError("Could not identify the ROM hash.")
    return match.group(1)


def make_pack(response, rom_hash, game_id):
    patch = response.get("PatchData")
    if not isinstance(patch, dict) or patch.get("ConsoleID") != 5 or patch.get("ID") != game_id:
        raise PreparationError("Downloaded data does not match the identified GBA game.")
    achievements = patch.get("Achievements")
    if not isinstance(achievements, list):
        raise PreparationError("Response does not contain achievement definitions.")
    rules = []
    for achievement in achievements:
        if not isinstance(achievement, dict):
            raise PreparationError("Malformed achievement definition.")
        identifier = achievement.get("ID")
        if isinstance(identifier, int) and identifier >= SERVICE_WARNING_ID:
            continue  # Service notices are not earned achievements.
        if achievement.get("Flags") != 3:  # Official/core achievements only.
            continue
        try:
            rules.append({
                "id": achievement["ID"], "title": achievement["Title"],
                "description": achievement["Description"], "points": achievement["Points"],
                "rule": achievement["MemAddr"], "badge": achievement.get("BadgeName"),
            })
        except KeyError:
            raise PreparationError("Incomplete achievement definition.") from None
    if not rules:
        raise PreparationError("No official achievements were returned for this game.")
    # Explicit field selection excludes credentials and unrelated server data.
    return {"version": 1, "console_id": 5, "game_id": game_id,
            "title": patch.get("Title"), "rom_hash": rom_hash, "achievements": rules}


def save_pack(pack, directory, rom, validate=inspect):
    directory.mkdir(parents=True, exist_ok=True)
    destination = directory / (pack["rom_hash"] + ".json")
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(mode="w", encoding="utf-8", dir=directory, suffix=".tmp", delete=False) as output:
            temporary = Path(output.name)
            json.dump(pack, output, ensure_ascii=False, indent=2)
            output.write("\n")
            output.flush()
            os.fsync(output.fileno())
        # Parse all rules with rcheevos and verify the exact ROM before replacing
        # an existing pack. The temporary file contains definitions only.
        validated_hash = validate(rom, temporary)
        if validated_hash != pack["rom_hash"]:
            raise PreparationError("The ROM changed during preparation. Existing pack kept.")
        os.replace(temporary, destination)
        temporary = None
        return destination
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("rom", type=Path, help="Uncompressed GBA ROM")
    parser.add_argument("--sd-root", type=Path, default=ROOT / "sdcard", help="SD card root (default: project sdcard)")
    args = parser.parse_args()
    rom = args.rom.resolve(strict=True)
    rom_hash = inspect(rom)
    print(f"ROM hash: {rom_hash}")
    resolved = request({"r": "gameid", "m": rom_hash})
    game_id = resolved.get("GameID")
    if not isinstance(game_id, int) or isinstance(game_id, bool) or game_id <= 0:
        raise PreparationError("This exact ROM is not supported by RetroAchievements.")
    print(f"Identified RetroAchievements game {game_id}.")
    if not sys.stdin.isatty():
        raise PreparationError("Run this interactively in your own Terminal to enter credentials privately.")
    username = input("RetroAchievements username: ").strip()
    password = getpass.getpass("RetroAchievements password (hidden): ")
    if not username or not password:
        raise PreparationError("Username and password are required.")
    login = request({"r": "login2", "u": username, "p": password})
    del password
    token = login.get("Token")
    if not isinstance(token, str) or not token:
        raise PreparationError("Login did not return a token.")
    username = login.get("User", username)
    response = request({"r": "patch", "u": username, "t": token, "m": rom_hash})
    del login, token
    pack = make_pack(response, rom_hash, game_id)
    destination = save_pack(pack, args.sd_root / "Achievements/GBA", rom)
    print(f"Saved {len(pack['achievements'])} validated achievement rules to {destination}")
    print("Preparation complete. Gameplay uses this local pack without network access.")


if __name__ == "__main__":
    try:
        main()
    except (PreparationError, OSError) as error:
        print(f"Preparation failed: {error}", file=sys.stderr)
        sys.exit(1)
    except (KeyboardInterrupt, EOFError):
        print("\nPreparation cancelled.", file=sys.stderr)
        sys.exit(1)
