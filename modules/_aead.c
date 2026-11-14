/* _aead: AES-GCM over OpenSSL's EVP interface, a CPython built-in module.
 *
 * zoraya-tg (docs/tasks/TG-03-tgd.md): tgclient's vault.py seals keys with
 * AES-GCM from the `cryptography` package, which is Rust and C and is not
 * built for Zoraya. OpenSSL's libcrypto is already linked into python3 (for
 * _ssl and _hashlib), so this module exposes the two calls vault.py needs; a
 * small `cryptography` stand-in (tgd/zoraya/cryptography) maps AESGCM onto it.
 *
 *   encrypt(key, nonce, data, aad=None) -> bytes   ciphertext + 16-byte tag
 *   decrypt(key, nonce, data, aad=None) -> bytes   raises InvalidTag
 *
 * key: 16, 24 or 32 bytes; nonce: 8 to 128 bytes (12 is the norm).
 */
#define PY_SSIZE_T_CLEAN
#include <Python.h>
#include <openssl/evp.h>

#define TAG 16

static PyObject *InvalidTag;

static const EVP_CIPHER *cipher_for(Py_ssize_t key_len) {
    switch (key_len) {
    case 16: return EVP_aes_128_gcm();
    case 24: return EVP_aes_192_gcm();
    case 32: return EVP_aes_256_gcm();
    default: return NULL;
    }
}

static int check_args(Py_buffer *key, Py_buffer *nonce, const EVP_CIPHER **c) {
    *c = cipher_for(key->len);
    if (*c == NULL) {
        PyErr_SetString(PyExc_ValueError, "AESGCM key must be 128, 192, or 256 bits.");
        return -1;
    }
    if (nonce->len < 8 || nonce->len > 128) {
        PyErr_SetString(PyExc_ValueError, "Nonce must be between 8 and 128 bytes");
        return -1;
    }
    if (key->len > INT_MAX || nonce->len > INT_MAX) {
        PyErr_SetString(PyExc_OverflowError, "too long");
        return -1;
    }
    return 0;
}

/* One call: encrypt (enc = 1) or decrypt (enc = 0). For decryption the data
 * ends with the tag. Returns a new bytes object or NULL with an exception. */
static PyObject *run(int enc, Py_buffer *key, Py_buffer *nonce, Py_buffer *data, Py_buffer *aad) {
    const EVP_CIPHER *c;
    if (check_args(key, nonce, &c) < 0)
        return NULL;
    Py_ssize_t body = data->len;
    if (!enc) {
        if (body < TAG) {
            PyErr_SetNone(InvalidTag);
            return NULL;
        }
        body -= TAG;
    }
    if (body > INT_MAX - TAG || (aad != NULL && aad->len > INT_MAX)) {
        PyErr_SetString(PyExc_OverflowError, "data too long");
        return NULL;
    }
    PyObject *out = PyBytes_FromStringAndSize(NULL, enc ? body + TAG : body);
    if (out == NULL)
        return NULL;
    unsigned char *o = (unsigned char *)PyBytes_AS_STRING(out);
    const unsigned char *in = (const unsigned char *)data->buf;
    int ok = 0, n = 0;
    EVP_CIPHER_CTX *ctx = NULL;

    Py_BEGIN_ALLOW_THREADS
    ctx = EVP_CIPHER_CTX_new();
    if (ctx != NULL
        && EVP_CipherInit_ex(ctx, c, NULL, NULL, NULL, enc) == 1
        && EVP_CIPHER_CTX_ctrl(ctx, EVP_CTRL_GCM_SET_IVLEN, (int)nonce->len, NULL) == 1
        && EVP_CipherInit_ex(ctx, NULL, NULL, key->buf, nonce->buf, enc) == 1
        && (aad == NULL || aad->len == 0
            || EVP_CipherUpdate(ctx, NULL, &n, aad->buf, (int)aad->len) == 1)
        && (body == 0 || EVP_CipherUpdate(ctx, o, &n, in, (int)body) == 1)
        && (enc || EVP_CIPHER_CTX_ctrl(ctx, EVP_CTRL_GCM_SET_TAG, TAG, (void *)(in + body)) == 1)
        && EVP_CipherFinal_ex(ctx, o + (body == 0 ? 0 : n), &n) == 1
        && (!enc || EVP_CIPHER_CTX_ctrl(ctx, EVP_CTRL_GCM_GET_TAG, TAG, o + body) == 1))
        ok = 1;
    EVP_CIPHER_CTX_free(ctx);
    Py_END_ALLOW_THREADS

    if (!ok) {
        Py_DECREF(out);
        if (ctx == NULL)
            return PyErr_NoMemory();
        if (enc)
            PyErr_SetString(PyExc_RuntimeError, "AES-GCM encryption failed");
        else
            PyErr_SetNone(InvalidTag);
        return NULL;
    }
    return out;
}

static PyObject *call(int enc, PyObject *args, PyObject *kwargs) {
    static char *names[] = {"key", "nonce", "data", "aad", NULL};
    Py_buffer key, nonce, data, aad = {0};
    PyObject *aad_obj = Py_None;
    if (!PyArg_ParseTupleAndKeywords(args, kwargs, "y*y*y*|O", names, &key, &nonce, &data, &aad_obj))
        return NULL;
    PyObject *r = NULL;
    int have_aad = aad_obj != Py_None;
    if (have_aad && PyObject_GetBuffer(aad_obj, &aad, PyBUF_SIMPLE) < 0)
        goto done;
    r = run(enc, &key, &nonce, &data, have_aad ? &aad : NULL);
    if (have_aad)
        PyBuffer_Release(&aad);
done:
    PyBuffer_Release(&key);
    PyBuffer_Release(&nonce);
    PyBuffer_Release(&data);
    return r;
}

static PyObject *aead_encrypt(PyObject *m, PyObject *args, PyObject *kwargs) {
    (void)m;
    return call(1, args, kwargs);
}

static PyObject *aead_decrypt(PyObject *m, PyObject *args, PyObject *kwargs) {
    (void)m;
    return call(0, args, kwargs);
}

static PyMethodDef methods[] = {
    {"encrypt", (PyCFunction)(void (*)(void))aead_encrypt, METH_VARARGS | METH_KEYWORDS,
     "encrypt(key, nonce, data, aad=None) -> ciphertext + tag (AES-GCM)"},
    {"decrypt", (PyCFunction)(void (*)(void))aead_decrypt, METH_VARARGS | METH_KEYWORDS,
     "decrypt(key, nonce, data, aad=None) -> plaintext; raises InvalidTag"},
    {NULL, NULL, 0, NULL},
};

static int exec_module(PyObject *m) {
    if (InvalidTag == NULL) {
        InvalidTag = PyErr_NewException("_aead.InvalidTag", NULL, NULL);
        if (InvalidTag == NULL)
            return -1;
    }
    return PyModule_AddObjectRef(m, "InvalidTag", InvalidTag);
}

static PyModuleDef_Slot slots[] = {
    {Py_mod_exec, exec_module},
    {0, NULL},
};

static struct PyModuleDef module = {
    PyModuleDef_HEAD_INIT, "_aead", "AES-GCM over OpenSSL (zoraya-tg)", 0, methods, slots,
    NULL, NULL, NULL,
};

PyMODINIT_FUNC PyInit__aead(void) { return PyModuleDef_Init(&module); }
