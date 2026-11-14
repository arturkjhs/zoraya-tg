"""tgd's entry script: /usr/lib/tgd/tgd.py, run as `python3 -I` (no environment, no user
site, no current directory on sys.path) by /bin/tgd on Zoraya (tgd/launcher.c) and by
out/host/bin/tgd on the host. Its library is the directory it is in."""
import os
import sys

_lib = os.path.dirname(os.path.realpath(__file__))
_paths = [_lib, os.path.join(_lib, "site")]
if sys.platform == "zoraya":
    _paths.append(os.path.join(_lib, "zoraya"))  # the cryptography stand-in over _aead
sys.path[:0] = _paths

from tgd.main import main  # noqa: E402

sys.exit(main())
