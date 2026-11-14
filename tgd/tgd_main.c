/* tgd's program on Zoraya: /bin/tgd, CPython (the same objects, modules and libraries as
 * zoraya-tg's python3) with a main that runs /usr/lib/tgd/tgd.py.
 *
 * Why not a script or an exec: zosh starts ELF images only (a `#!` file is "not a valid
 * argument"), and a process that execs another image has no random source (CPython stops in
 * _Py_HashRandomization_Init), as a fork child has none until stage 10's 10h. So tgd is the
 * program the shell starts, and gets the line's grants and getrandom itself.
 *
 * Below: Zoraya's Programs/python.c for __zoraya__ (ports/patches/python/0004: the
 * interpreter on a thread with a 2 MiB stack, signals blocked in the main thread), with the
 * script fixed: copied once
 * (CPython 3.15.2 with Zoraya's patch 0004) and kept here; recheck when the recipe moves. */
#include "Python.h"

#ifndef TGD_SCRIPT
#define TGD_SCRIPT "/usr/lib/tgd/tgd.py"
#endif

/* TASK-27 9c, ADR-0320 item 5: a Zoraya process starts on a 64 KiB stack
   (zoraya_abi::spawn::USER_STACK_SIZE), where CPython's own stack-overflow
   guard (_PyOS_STACK_MARGIN_BYTES * 2 = 32 KiB reserve) leaves almost nothing
   and the interpreter fails at its first import. The interpreter runs on a
   thread with a larger stack (ZORAYA_PY_STACK); the main thread waits for it.
   The thread ends the process itself with exit(status): in a fork child
   (ADR-0306) the wrapper thread is the only one left in musl's list, and its
   return would end the process with status 0 whatever the interpreter
   returned.
   The main thread blocks every signal: it would otherwise be the lowest-tid
   thread that waits with the signal unblocked (ADR-0304 rules 3 and 4) and
   take SIGINT itself, so the interpreter's own thread, the one CPython's
   handler and its EINTR checks run on, would see no Ctrl-C while blocked in
   time.sleep, input or a socket read. It blocks them before the thread is
   created, and the thread restores the mask it was started with, so no
   signal falls between the two. */
#include <pthread.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>

#define ZORAYA_PY_STACK (2u << 20)

struct zoraya_main { int argc; char **argv; sigset_t mask; };

static void *
zoraya_main_thread(void *p)
{
    struct zoraya_main *m = p;
    if (pthread_sigmask(SIG_SETMASK, &m->mask, NULL) != 0) {
        fputs("tgd: cannot restore the signal mask\n", stderr);
        exit(1);
    }
    /* tgd: python3 -I /usr/lib/tgd/tgd.py ARGS... */
    char **args = calloc((size_t)m->argc + 3, sizeof *args);
    if (args == NULL) {
        fputs("tgd: out of memory\n", stderr);
        exit(1);
    }
    args[0] = m->argv[0];
    args[1] = "-I";
    args[2] = TGD_SCRIPT;
    for (int i = 1; i < m->argc; i++)
        args[i + 2] = m->argv[i];
    exit(Py_BytesMain(m->argc + 2, args));
    return NULL;
}

int
main(int argc, char **argv)
{
    struct zoraya_main m = { argc, argv };
    sigset_t all;
    pthread_attr_t attr;
    pthread_t t;
    sigfillset(&all);
    if (pthread_sigmask(SIG_BLOCK, &all, &m.mask) != 0) {
        fputs("tgd: cannot block signals in the main thread\n", stderr);
        return 1;
    }
    if (pthread_attr_init(&attr) != 0
        || pthread_attr_setstacksize(&attr, ZORAYA_PY_STACK) != 0
        || pthread_create(&t, &attr, zoraya_main_thread, &m) != 0) {
        /* Not a fallback to the 64 KiB stack: it only ends in a misleading
           RecursionError. */
        fputs("tgd: cannot start the interpreter thread "
              "(not enough memory for its stack?)\n", stderr);
        return 1;
    }
    /* The thread ends the process with exit(); a join that returns is a bug
       of the thread library, not an exit status. */
    pthread_join(t, NULL);
    abort();
}
