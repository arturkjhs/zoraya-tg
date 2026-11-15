# TG-03: a runner script with @GRANT@ replaced by the report's line, and every key line longer
# than 64 bytes sent in pieces 300 ms apart: the console's input buffer drops what a whole
# line typed at once overflows ("[input: N bytes dropped]"), which a person typing never hits.
# Usage: python3 expand_script.py SCRIPT GRANT > OUT
import sys

script, grant = sys.argv[1], sys.argv[2]
for line in open(script, encoding="utf-8").read().splitlines():
    line = line.replace("@GRANT@", grant)
    if not line.startswith("> ") or len(line) <= 66:
        print(line)
        continue
    keys, enter = (line[2:-2], "\\r") if line.endswith("\\r") else (line[2:], "")
    assert "\\" not in keys, "an escape inside a long line would be split"
    pieces = [keys[i:i + 64] for i in range(0, len(keys), 64)]
    pieces[-1] += enter
    for n, piece in enumerate(pieces):
        if n:
            print("~ 300 @@no such text@@")
        # blanks as \x20, so none is lost at either end of a piece
        print("> " + piece.replace(" ", "\\x20"))
