# TG-03: TDLib's databases on Zoraya: setTdlibParameters with a database directory opens
# the binlog (fcntl record lock, ADR-0319) and the SQLite database in WAL mode (patch 0004:
# the unix-excl VFS, WAL index in heap memory), writes them, and reaches
# authorizationStateWaitPhoneNumber; closed and opened again (second run), it finds them.
# No account and no network: a placeholder api id that never leaves the guest (the line has
# no +net). Usage: python3 tdjson_db.py DIR [VERBOSITY]  (a TDLib log in DIR/td.log)
import _tdjson, json, os, sys, time

base = sys.argv[1]
db = os.path.join(base, "db")
first = not os.path.exists(os.path.join(db, "td_test.binlog"))
print("run", "first" if first else "again")
verbosity = int(sys.argv[2]) if len(sys.argv) > 2 else 1
os.makedirs(base, exist_ok=True)
_tdjson.execute({"@type": "setLogVerbosityLevel", "new_verbosity_level": verbosity})
if verbosity > 1:
    _tdjson.execute({"@type": "setLogStream", "log_stream": {
        "@type": "logStreamFile", "path": os.path.join(base, "td.log"), "max_file_size": 1 << 24}})
cid = _tdjson.create_client_id()
_tdjson.send(cid, {"@type": "getOption", "name": "version"})
states = []
closing = False
deadline = time.monotonic() + 120
while time.monotonic() < deadline:
    text = _tdjson.receive(1.0)
    if text is None:
        continue
    event = json.loads(text)
    if event.get("@type") == "error":
        print("error", event.get("code"), event.get("message"))
    if event.get("@type") != "updateAuthorizationState":
        continue
    state = event["authorization_state"]["@type"]
    states.append(state)
    print("state", state)
    if state == "authorizationStateWaitTdlibParameters":
        _tdjson.send(cid, {
            "@type": "setTdlibParameters", "use_test_dc": True,
            "database_directory": db, "files_directory": os.path.join(base, "files"),
            "database_encryption_key": "", "use_file_database": True,
            "use_chat_info_database": True, "use_message_database": True,
            "use_secret_chats": True, "api_id": 1, "api_hash": "0" * 32,
            "system_language_code": "en", "device_model": "smoke", "system_version": "",
            "application_version": "smoke"})
    elif state == "authorizationStateWaitPhoneNumber" and not closing:
        _tdjson.send(cid, {"@type": "close"})
        closing = True
    elif state == "authorizationStateClosed":
        break
names = sorted(os.listdir(db)) if os.path.isdir(db) else []
print("files", " ".join(names))
sizes = {n: os.path.getsize(os.path.join(db, n)) for n in names}
ok = ("authorizationStateWaitPhoneNumber" in states and states[-1:] == ["authorizationStateClosed"]
      and sizes.get("td_test.binlog", 0) > 0 and sizes.get("db_test.sqlite", 0) > 0
      and "db_test.sqlite-shm" not in names)
print("binlog", sizes.get("td_test.binlog", 0) > 0, "sqlite", sizes.get("db_test.sqlite", 0) > 0,
      "no shm", "db_test.sqlite-shm" not in names)
print("db done" if ok else "db FAILED")
