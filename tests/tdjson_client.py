# TG-02 smoke test, part 2: a TDLib client made through _tdjson reaches
# authorizationStateWaitTdlibParameters, and answers getOption("version").
# No account, no parameters, no network: the client is closed before it would
# need any. Prints one line per step; "client done" at the end.
import _tdjson, json, time

print("log", _tdjson.execute({"@type": "setLogVerbosityLevel", "new_verbosity_level": 1}))
cid = _tdjson.create_client_id()
print("client id", cid)
_tdjson.send(cid, {"@type": "getOption", "name": "version", "@extra": "version"})
waiting = version = closed = False
closing = False
deadline = time.monotonic() + 120
while time.monotonic() < deadline and not closed:
    text = _tdjson.receive(1.0)
    if text is None:
        continue
    event = json.loads(text)
    kind = event.get("@type")
    if kind == "updateAuthorizationState":
        state = event["authorization_state"]["@type"]
        print("recv updateAuthorizationState", state)
        if state == "authorizationStateWaitTdlibParameters":
            waiting = True
        if state == "authorizationStateClosed":
            closed = True
    elif event.get("@extra") == "version":
        print("recv", kind, event.get("value"))
        version = kind == "optionValueString"
    else:
        print("recv", kind)
    if waiting and version and not closing:
        _tdjson.send(cid, {"@type": "close"})
        closing = True
print("waiting", waiting, "version", version, "closed", closed)
print("client done" if waiting and version and closed else "client FAILED")
