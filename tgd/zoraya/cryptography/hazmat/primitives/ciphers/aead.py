from __future__ import annotations

import os

import _aead


class AESGCM:
    """cryptography's AESGCM: encrypt(nonce, data, aad) -> ciphertext + tag."""

    def __init__(self, key: bytes) -> None:
        if len(key) not in (16, 24, 32):
            raise ValueError("AESGCM key must be 128, 192, or 256 bits.")
        self._key = bytes(key)

    @classmethod
    def generate_key(cls, bit_length: int) -> bytes:
        if bit_length not in (128, 192, 256):
            raise ValueError("bit_length must be 128, 192, or 256")
        return os.urandom(bit_length // 8)

    def encrypt(self, nonce: bytes, data: bytes, associated_data: bytes | None) -> bytes:
        return _aead.encrypt(self._key, nonce, data, associated_data)

    def decrypt(self, nonce: bytes, data: bytes, associated_data: bytes | None) -> bytes:
        return _aead.decrypt(self._key, nonce, data, associated_data)
