name     tgd
summary  tgclient's backend: TDLib, the stores, the services
usage    tgd [--window] [--json] [--test-dc] [--no-probe]
option   --window  serve the window tg on a socket file in its own directory
option   --json  the line protocol on the input and output
option   --test-dc  Telegram's test data centres
option   --no-probe  start without the network probe
option   --grant  print the line that starts tgd for one run, and end
option   --install-line  print the line that installs tgd with its grants, and end
option   --version  print tgd's version
cwd      none
exit     0  ended
exit     3  no network for Telegram: the saved grants are missing
exit     4  its own directory cannot be written
exit     5  another tgd uses the same data
exit     6  the window's socket could not be made
exit     7  the window's socket stopped
exit     8  the api id and hash are not set, and it runs in the background
---
tgd keeps the login, the database and the AI's key in its own directory (~/.config/tgd, which
is /data/ROOM/tgd), which no other program reads. With --window it makes the socket
window.sock there, and the window tg reaches that one file through its saved +sock grant.
It is not given the current directory (cwd none): its own directory and the network are all
it needs. Ctrl-C ends it.

EXAMPLES
  tgd --install-line
  tgd --window

SEE ALSO
  tg, programs
