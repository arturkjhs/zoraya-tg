name     tg
summary  the Telegram window, over tgd's socket
usage    tg [SOCKET]
operand  [SOCKET]  text  tgd's socket file; /data/home/tgd/window.sock if none is given
window   Telegram
cwd      none
exit     0  the window was closed
exit     1  no window could be opened
---
tg is a window: the chat list, the open chat, the composer, search, the AI summary and the
login. It holds no network, no directory (not even the current one: cwd none) and none of
tgd's files: only the one socket file its saved grant names, through which tgd serves it
(protocol 3).

Start tgd first, in a terminal of its own (tgd --window), then tg in another.

EXAMPLES
  install /packages/tg +sock /data/home/tgd/window.sock
  tg

SEE ALSO
  tgd, programs
