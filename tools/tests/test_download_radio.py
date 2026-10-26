"""Exercise BaseOS connection/cleanup with mocked commands and no real radio."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().parents[2] / 'card/System/slotdownload.sh'

class DownloadRadioTests(unittest.TestCase):
    def test_connect_failure_and_disconnect_cleanup(self):
        for dhcp_fails in [False, True]:
            with self.subTest(dhcp_fails=dhcp_fails), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                run = root / 'run'
                (run / 'slot-download/wpa').mkdir(parents=True)
                (run / 'slot-download/network.conf').write_text('test config')
                (root / 'net/wlan0').mkdir(parents=True)
                tools = root / 'bin'
                tools.mkdir()
                commands = {
                    'ip': 'echo "ip $*" >> "$TEST_LOG"\n',
                    'rfkill': 'exit 0\n',
                    'wpa_cli': 'echo "wpa $*" >> "$TEST_LOG"\ncase "$*" in *status*) echo wpa_state=COMPLETED;; esac\n',
                    'wpa_supplicant': 'echo "start $*" >> "$TEST_LOG"\n',
                    'udhcpc': 'echo dhcp >> "$TEST_LOG"\nexit "${TEST_DHCP_FAIL:-0}"\n',
                }
                for name, body in commands.items():
                    path = tools / name
                    path.write_text('#!/bin/sh\n' + body)
                    path.chmod(0o755)
                env = dict(os.environ, PATH=str(tools)+os.pathsep+os.environ['PATH'],
                           SLOT_RUN=str(run), AGS_NET_SYS=str(root/'net'), TEST_LOG=str(root/'commands.log'),
                           TEST_DHCP_FAIL='1' if dhcp_fails else '0')
                result = subprocess.run(['sh', str(SCRIPT), 'up'], env=env, capture_output=True, timeout=5)
                self.assertEqual(result.returncode, 1 if dhcp_fails else 0)
                subprocess.run(['sh', str(SCRIPT), 'down'], env=env, check=True, capture_output=True, timeout=5)
                log = (root/'commands.log').read_text()
                self.assertIn('dhcp', log)
                self.assertIn('terminate', log)
                self.assertIn('addr flush dev wlan0', log)
                self.assertIn('link set wlan0 down', log)
                self.assertFalse((run/'slot-download/network.conf').exists())

if __name__ == '__main__':
    unittest.main()
