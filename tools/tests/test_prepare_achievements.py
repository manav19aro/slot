import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location("prepare", Path(__file__).parents[1] / "prepare_achievements.py")
prepare = importlib.util.module_from_spec(spec)
spec.loader.exec_module(prepare)
HASH = "f877b87f140945ad93cfce50411507c0"


class PreparationTests(unittest.TestCase):
    def response(self):
        return {"Token": "must-not-be-saved", "PatchData": {
            "ID": 4969, "ConsoleID": 5, "Title": "Test game", "Achievements": [
                {"ID": 1, "Flags": 3, "Title": "Test", "Description": "Test rule", "Points": 1, "MemAddr": "0xH008000=3"},
                {"ID": 2, "Flags": 5, "Title": "Unofficial"},
            ]}}

    def test_official_rules_only_and_no_credentials(self):
        pack = prepare.make_pack(self.response(), HASH, 4969)
        self.assertEqual([a["id"] for a in pack["achievements"]], [1])
        self.assertNotIn("must-not-be-saved", json.dumps(pack))
        self.assertEqual(pack["rom_hash"], HASH)

    def test_service_warnings_excluded_but_zero_point_achievements_kept(self):
        response = self.response()
        normal = dict(response["PatchData"]["Achievements"][0], Points=0)
        response["PatchData"]["Achievements"] = [normal] + [
            dict(normal, ID=identifier, Title="Service notice")
            for identifier in [101000001, 101000002]
        ]
        pack = prepare.make_pack(response, HASH, 4969)
        self.assertEqual([a["id"] for a in pack["achievements"]], [1])
        self.assertEqual(pack["achievements"][0]["points"], 0)

    def test_wrong_game_or_console_rejected(self):
        for field, value in [("ID", 1), ("ConsoleID", 4)]:
            response = self.response()
            response["PatchData"][field] = value
            with self.assertRaises(prepare.PreparationError):
                prepare.make_pack(response, HASH, 4969)

    def test_failed_validation_keeps_previous_pack_and_cleans_temp(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            path = root / (HASH + ".json")
            path.write_text("previous pack")
            def reject(*args):
                raise prepare.PreparationError("bad rule")
            with self.assertRaises(prepare.PreparationError):
                prepare.save_pack(prepare.make_pack(self.response(), HASH, 4969), root, root / "game.gba", validate=reject)
            self.assertEqual(path.read_text(), "previous pack")
            self.assertEqual(list(root.iterdir()), [path])

    def test_success_replaces_only_after_validation(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            pack = prepare.make_pack(self.response(), HASH, 4969)
            path = prepare.save_pack(pack, root, root / "game.gba", validate=lambda *_: HASH)
            self.assertEqual(json.loads(path.read_text()), pack)


if __name__ == "__main__":
    unittest.main()
