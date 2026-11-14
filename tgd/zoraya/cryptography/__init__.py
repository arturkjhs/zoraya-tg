"""A stand-in for the two names of the `cryptography` package that tgclient's vault.py uses
(AESGCM and InvalidTag), over the built-in module _aead (OpenSSL). Only on Zoraya: on the
host the real package is used. zoraya-tg, docs/tasks/TG-03-tgd.md."""
