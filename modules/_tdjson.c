/* _tdjson: TDLib's JSON interface (td/telegram/td_json_client.h) as a CPython
 * built-in module, linked statically with TDLib into python3.
 *
 *   create_client_id() -> int
 *   send(client_id, request) -> None
 *   receive(timeout) -> str or None
 *   execute(request) -> str or None
 *
 * A request is a str (JSON text), bytes (UTF-8 JSON) or a dict, which is
 * serialized with json.dumps. receive() waits at most `timeout` seconds with
 * the GIL released; TDLib allows one td_receive at a time, so concurrent
 * callers are serialized by a lock of this module. The strings TDLib returns
 * live until its next call of the same kind on that thread: each is copied
 * into a Python str before any other TDLib call can happen.
 */
#define PY_SSIZE_T_CLEAN
#include <Python.h>

#include <td/telegram/td_json_client.h>

static PyThread_type_lock receive_lock;

/* The request as a new bytes object of UTF-8 JSON, or NULL with an exception. */
static PyObject *request_bytes(PyObject *request)
{
	if (PyBytes_Check(request)) {
		Py_INCREF(request);
		return request;
	}
	if (PyUnicode_Check(request))
		return PyUnicode_AsUTF8String(request);
	if (PyDict_Check(request)) {
		PyObject *json = PyImport_ImportModule("json");
		if (json == NULL)
			return NULL;
		PyObject *text = PyObject_CallMethod(json, "dumps", "O", request);
		Py_DECREF(json);
		if (text == NULL)
			return NULL;
		PyObject *bytes = PyUnicode_AsUTF8String(text);
		Py_DECREF(text);
		return bytes;
	}
	PyErr_Format(PyExc_TypeError, "request must be str, bytes or dict, not %.100s", Py_TYPE(request)->tp_name);
	return NULL;
}

/* A NUL inside the request would cut it short in TDLib's C string. */
static const char *request_text(PyObject *bytes)
{
	const char *text = PyBytes_AS_STRING(bytes);
	if ((Py_ssize_t)strlen(text) != PyBytes_GET_SIZE(bytes)) {
		PyErr_SetString(PyExc_ValueError, "request contains a NUL byte");
		return NULL;
	}
	return text;
}

static PyObject *answer(const char *result)
{
	if (result == NULL)
		Py_RETURN_NONE;
	return PyUnicode_DecodeUTF8(result, (Py_ssize_t)strlen(result), "replace");
}

static PyObject *tdjson_create_client_id(PyObject *self, PyObject *noargs)
{
	(void)self;
	(void)noargs;
	int id;
	Py_BEGIN_ALLOW_THREADS
	id = td_create_client_id();
	Py_END_ALLOW_THREADS
	return PyLong_FromLong(id);
}

static PyObject *tdjson_send(PyObject *self, PyObject *args)
{
	(void)self;
	int client_id;
	PyObject *request;
	if (!PyArg_ParseTuple(args, "iO:send", &client_id, &request))
		return NULL;
	PyObject *bytes = request_bytes(request);
	if (bytes == NULL)
		return NULL;
	const char *text = request_text(bytes);
	if (text == NULL) {
		Py_DECREF(bytes);
		return NULL;
	}
	Py_BEGIN_ALLOW_THREADS
	td_send(client_id, text);
	Py_END_ALLOW_THREADS
	Py_DECREF(bytes);
	Py_RETURN_NONE;
}

static PyObject *tdjson_receive(PyObject *self, PyObject *args)
{
	(void)self;
	double timeout;
	if (!PyArg_ParseTuple(args, "d:receive", &timeout))
		return NULL;
	if (!(timeout >= 0.0)) {
		PyErr_SetString(PyExc_ValueError, "timeout must be a non-negative number");
		return NULL;
	}
	const char *result;
	Py_BEGIN_ALLOW_THREADS
	PyThread_acquire_lock(receive_lock, WAIT_LOCK);
	result = td_receive(timeout);
	Py_END_ALLOW_THREADS
	/* Copied while the lock still keeps any other td_receive out. */
	PyObject *value = answer(result);
	PyThread_release_lock(receive_lock);
	return value;
}

static PyObject *tdjson_execute(PyObject *self, PyObject *request)
{
	(void)self;
	PyObject *bytes = request_bytes(request);
	if (bytes == NULL)
		return NULL;
	const char *text = request_text(bytes);
	if (text == NULL) {
		Py_DECREF(bytes);
		return NULL;
	}
	/* Synchronous and short; the GIL stays held so that the thread-local
	 * result is copied before this thread can call TDLib again. */
	PyObject *value = answer(td_execute(text));
	Py_DECREF(bytes);
	return value;
}

static PyMethodDef tdjson_methods[] = {
	{"create_client_id", tdjson_create_client_id, METH_NOARGS,
	 "create_client_id() -> int: a new TDLib client; it exists once a request is sent to it."},
	{"send", tdjson_send, METH_VARARGS,
	 "send(client_id, request): send a request (str, bytes or dict) to a client."},
	{"receive", tdjson_receive, METH_VARARGS,
	 "receive(timeout) -> str or None: the next update or answer of any client, waiting at most timeout seconds."},
	{"execute", tdjson_execute, METH_O,
	 "execute(request) -> str or None: a synchronous TDLib request (str, bytes or dict)."},
	{NULL, NULL, 0, NULL},
};

static int tdjson_exec(PyObject *module)
{
	if (receive_lock == NULL) {
		receive_lock = PyThread_allocate_lock();
		if (receive_lock == NULL) {
			PyErr_NoMemory();
			return -1;
		}
	}
	return PyModule_AddStringConstant(module, "__version__", "1");
}

static PyModuleDef_Slot tdjson_slots[] = {
	{Py_mod_exec, tdjson_exec},
	{Py_mod_multiple_interpreters, Py_MOD_PER_INTERPRETER_GIL_SUPPORTED},
	{0, NULL},
};

static struct PyModuleDef tdjson_module = {
	PyModuleDef_HEAD_INIT,
	.m_name = "_tdjson",
	.m_doc = "TDLib's JSON client interface (td_json_client.h).",
	.m_size = 0,
	.m_methods = tdjson_methods,
	.m_slots = tdjson_slots,
};

PyMODINIT_FUNC PyInit__tdjson(void)
{
	return PyModuleDef_Init(&tdjson_module);
}
