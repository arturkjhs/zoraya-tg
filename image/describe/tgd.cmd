name     tgd
summary  a Telegram client's backend: TDLib, its stores and services, a text front end or JSON lines
usage    tgd [--test-dc] [--json] [--data DIR] [--no-probe] [--td-log N]
usage    tgd --grant [--test-dc]
usage    tgd --version
program  /bin/tgd
option   --test-dc  Telegram's test data centres (a separate database in the data directory)
option   --json  the line protocol on standard input and output, for a front end
option   --data DIR write  the data directory instead of ~/tg
option   --no-probe  do not test the network at the start
option   --td-log N text  TDLib's log verbosity (td.log in the data directory)
option   --grant  print the line that starts tgd with its network and its data, and exit
option   --version  print the version and exit
exit     0  quit
exit     3  the network is not given (the line without +net); the line that gives it is printed
exit     4  the data directory cannot be written (+w ~/tg is not on the line)
exit     5  another tgd uses the data directory
---
tgd is the backend of a Telegram client (TDLib 1.8.70 inside one program with CPython 3.15), as a POSIX
program: /bin, /usr (its code in /usr/lib/tgd) and /etc to read, /dev/null and a /tmp of its own, the current
directory to read, and what the line's +w and +net words designate. Its data directory, ~/tg unless --data names
another, holds TDLib's database, the api id and hash it was given and the AI key, so it needs +w ~/tg on the line
until a directory of its own per program exists (ADR-0341). Telegram's data centres are reached by address, on
port 443, IPv4 only: the line names the ranges of Telegram's published list, which `tgd --grant` prints, and
openrouter.ai:443 for summaries. Nothing else is reached.

The api id and hash, the phone number, the login code and the 2FA password are typed at tgd's own prompt (login)
or given by a front end over the protocol; tgd keeps them in its data directory only, never prints them, and its
log carries none of them and no message text. Typed input is shown as it is typed here (there is no terminal from
zosh, ADR-0153, so tgd cannot turn echo off).

Until this description is part of the image, the line runs the same program as python3 /usr/lib/tgd/tgd.py,
which python3's description gives the line's +net words.

EXAMPLES
  tgd --grant
  +net 149.154.160.0/20:443 +net 91.108.4.0/22:443 +net openrouter.ai:443 +w ~/tg tgd
  +w ~/tg tgd --json --no-probe

SEE ALSO
  grants, python3
