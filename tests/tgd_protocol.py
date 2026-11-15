# TG-03: tgd's line protocol on the host, with no account (make test-tgd-host): a fresh data
# directory, no credentials (an api file that does not exist), so TDLib is never given
# parameters and nothing reaches Telegram. Checks hello, need_credentials, a reply, an error
# reply, a refused answer, quit, bye and exit status 0; then the lock (a second tgd on the
# same directory is refused) and the --grant line. Usage: python3 tgd_protocol.py TGD
import json, subprocess, sys, tempfile, os

tgd = sys.argv[1]
failures = []


def check(what, ok):
    print(("ok   " if ok else "FAIL ") + what)
    if not ok:
        failures.append(what)


with tempfile.TemporaryDirectory() as data:
    args = [tgd, "--json", "--test-dc", "--no-probe", "--data", data,
            "--api-file", os.path.join(data, "no-such-file")]
    p = subprocess.Popen(args, stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)

    def read():
        line = p.stdout.readline()
        return json.loads(line) if line else None

    def send(obj):
        p.stdin.write(json.dumps(obj) + "\n")
        p.stdin.flush()

    hello = read()
    check("hello", hello and hello.get("ev") == "hello" and hello.get("binding") == "ctypes"
          and hello.get("tdlib", "").startswith("1.8."))
    state = read()
    check("need_credentials", state and state.get("state") == "need_credentials")
    send({"op": "version", "id": 1})
    r = read()
    check("version reply", r and r.get("re") == 1 and r.get("ok") is True and r.get("protocol") == 1)
    send({"op": "nonsense", "id": 2})
    r = read()
    check("error reply", r and r.get("re") == 2 and r.get("ok") is False)
    send({"op": "auth.code", "id": 3, "value": "12345"})
    r = read()
    check("answer refused when nothing is asked", r and r.get("re") == 3 and r.get("ok") is False)
    send({"op": "chats.list", "id": 4})
    r = read()
    check("chats refused before login", r and r.get("re") == 4 and r.get("ok") is False)
    second = subprocess.run(args, input="", capture_output=True, text=True, timeout=60)
    check("second tgd refused by the lock", second.returncode == 5 and '"lock"' in second.stdout)
    send({"op": "quit", "id": 5})
    events = [read(), read()]
    check("quit reply and bye", events[0] and events[0].get("re") == 5
          and events[1] and events[1].get("ev") == "bye")
    check("exit status 0", p.wait(timeout=60) == 0)
    check("lock released", not os.path.exists(os.path.join(data, "tgd.lock")))
grant = subprocess.run([tgd, "--grant"], capture_output=True, text=True, timeout=60).stdout
check("grant line", grant.startswith("+net 91.108.56.0/22:443 ") and "+w ~/tg python3 /usr/lib/tgd/tgd.py" in grant)
print("protocol done" if not failures else f"protocol FAILED: {len(failures)}")
sys.exit(1 if failures else 0)
