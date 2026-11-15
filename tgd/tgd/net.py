"""The network tgd needs: Telegram's IPv4 ranges (pinned, telegram-cidr.txt), OpenRouter by
name, and on Zoraya the line that grants them. A probe at start tells a refused network (no
`+net` for the data centres: EACCES on Zoraya) from one that is merely unreachable."""

from __future__ import annotations

import errno
import ipaddress
import socket
from pathlib import Path

CIDR_FILE = Path(__file__).with_name("telegram-cidr.txt")
PORT = 443  # TDLib also tries 80 and 5222; 443 is enough, and one port per +net word
OPENROUTER = "openrouter.ai"

# A data centre each kind of DC answers on (DC 2; both inside 149.154.160.0/20).
PROBE = {False: "149.154.167.50", True: "149.154.167.40"}


def ipv4_ranges() -> list[str]:
    ranges = []
    for line in CIDR_FILE.read_text(encoding="ascii").splitlines():
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        net = ipaddress.ip_network(line)
        if net.version == 4:  # Zoraya has no IPv6 (ADR-0225)
            ranges.append(str(net))
    return ranges


COMMAND = "python3 /usr/lib/tgd/tgd.py"


def grant_line(data: str = "~/tg", test_dc: bool = False, ai: bool = True) -> str:
    words = [f"+net {r}:{PORT}" for r in ipv4_ranges()]
    if ai:
        words.append(f"+net {OPENROUTER}:443")
    words.append(f"+w {data}")
    # python3 and not /bin/tgd: zosh gives a POSIX program a network connection only under a
    # description the image carries (descriptions::POSIX), and the image describes python3,
    # not tgd. /bin/tgd is the same program; it runs without +net (the refusal, the database).
    words.append(COMMAND + (" --test-dc" if test_dc else ""))
    return " ".join(words)


class Refused(Exception):
    pass


def probe(test_dc: bool, timeout: float = 5.0) -> str:
    """'ok', or why not (a reason that is no refusal: the network may come later).
    Raises Refused when the system refuses the connection itself."""
    address = PROBE[test_dc]
    sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    sock.settimeout(timeout)
    try:
        sock.connect((address, PORT))
        return "ok"
    except PermissionError as e:
        raise Refused(f"connecting to {address}:{PORT} was refused ({e.strerror})") from None
    except OSError as e:
        if e.errno in (errno.EACCES, errno.EPERM):
            raise Refused(f"connecting to {address}:{PORT} was refused ({e.strerror})") from None
        return f"{address}:{PORT} not reachable now ({e.strerror or type(e).__name__})"
    finally:
        sock.close()
