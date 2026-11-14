/* Names TDLib references that Zoraya's libc.a does not export (TG-02). Linked
 * into the python3 of zoraya-tg from libtdshim.a, never into Zoraya's libc.a.
 * Each is built on what libc.a does export, and each is the subject of a
 * generic request in docs/requests.md: when libposix has the name, its entry
 * here goes.
 *
 * poll and select: over pselect, which libposix implements for pipes,
 * sockets, files and terminals (ADR-0106); pselect takes at most 1024
 * descriptors (an fd_set), so does this poll.
 *
 * pread and pwrite: lseek, read or write, lseek back. Not atomic against a
 * plain read or write of the same descriptor by another thread (TDLib's
 * FileFd does not mix them on one descriptor); calls of these two are
 * serialized among themselves by a lock.
 *
 * usleep: nanosleep. mkdtemp: mkdir of random names (getrandom).
 * in6addr_any: the constant (Zoraya is IPv4 only, ADR-0225; TDLib names it
 * for an address family it then does not get).
 *
 * bind, listen, accept: a listening socket (TDLib's ServerSocketFd, which the
 * JSON client never makes): they fail with EOPNOTSUPP.
 *
 * __libc_current_sigrtmin: musl's SIGRTMIN, named by a function of TDLib's
 * thread class that nothing in the library calls; musl's value.
 */
#include <errno.h>
#include <netinet/in.h>
#include <string.h>
#include <sys/random.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <poll.h>
#include <pthread.h>
#include <signal.h>
#include <stddef.h>
#include <sys/select.h>
#include <sys/types.h>
#include <time.h>
#include <unistd.h>

int poll(struct pollfd *fds, nfds_t nfds, int timeout)
{
	fd_set rset, wset;
	int maxfd = -1;
	FD_ZERO(&rset);
	FD_ZERO(&wset);
	int invalid = 0;
	for (nfds_t i = 0; i < nfds; i++) {
		fds[i].revents = 0;
		int fd = fds[i].fd;
		if (fd < 0)
			continue;
		if (fd >= FD_SETSIZE) {
			fds[i].revents = POLLNVAL;
			invalid++;
			continue;
		}
		if (fds[i].events & (POLLIN | POLLRDNORM | POLLPRI))
			FD_SET(fd, &rset);
		if (fds[i].events & (POLLOUT | POLLWRNORM))
			FD_SET(fd, &wset);
		/* An fd asked for nothing still reports a hang-up: look at its read end. */
		if (!(fds[i].events & (POLLIN | POLLRDNORM | POLLPRI | POLLOUT | POLLWRNORM)))
			FD_SET(fd, &rset);
		if (fd > maxfd)
			maxfd = fd;
	}
	struct timespec ts, *tp = NULL;
	if (invalid > 0)
		timeout = 0;
	if (timeout >= 0) {
		ts.tv_sec = timeout / 1000;
		ts.tv_nsec = (long)(timeout % 1000) * 1000000L;
		tp = &ts;
	}
	int n = pselect(maxfd + 1, &rset, &wset, NULL, tp, NULL);
	if (n < 0) {
		if (errno == EBADF) {
			/* Find which: each descriptor alone, at once. */
			int count = 0;
			for (nfds_t i = 0; i < nfds; i++) {
				int fd = fds[i].fd;
				if (fd < 0 || fd >= FD_SETSIZE)
					continue;
				fd_set one;
				FD_ZERO(&one);
				FD_SET(fd, &one);
				struct timespec zero = {0, 0};
				if (pselect(fd + 1, &one, NULL, NULL, &zero, NULL) < 0 && errno == EBADF) {
					fds[i].revents = POLLNVAL;
					count++;
				}
			}
			if (count + invalid > 0)
				return count + invalid;
			errno = EBADF;
		}
		return -1;
	}
	int ready = invalid;
	for (nfds_t i = 0; i < nfds; i++) {
		int fd = fds[i].fd;
		if (fd < 0 || fd >= FD_SETSIZE)
			continue;
		short ev = 0;
		if (FD_ISSET(fd, &rset)) {
			/* Readable or closed by the peer: pselect does not say which;
			 * the read that follows tells (0 bytes is the end). */
			ev |= fds[i].events & (POLLIN | POLLRDNORM | POLLPRI);
			if (ev == 0)
				ev = POLLHUP;
		}
		if (FD_ISSET(fd, &wset))
			ev |= fds[i].events & (POLLOUT | POLLWRNORM);
		fds[i].revents = ev;
		if (ev)
			ready++;
	}
	return ready;
}

int select(int nfds, fd_set *readfds, fd_set *writefds, fd_set *exceptfds, struct timeval *timeout)
{
	struct timespec ts, *tp = NULL;
	if (timeout) {
		ts.tv_sec = timeout->tv_sec;
		ts.tv_nsec = timeout->tv_usec * 1000L;
		tp = &ts;
	}
	return pselect(nfds, readfds, writefds, exceptfds, tp, NULL);
}

static pthread_mutex_t positioned = PTHREAD_MUTEX_INITIALIZER;

static ssize_t positioned_io(int fd, void *buf, size_t count, off_t offset, int writing)
{
	if (offset < 0) {
		errno = EINVAL;
		return -1;
	}
	pthread_mutex_lock(&positioned);
	off_t here = lseek(fd, 0, SEEK_CUR);
	if (here < 0 || lseek(fd, offset, SEEK_SET) < 0) {
		int e = errno;
		pthread_mutex_unlock(&positioned);
		errno = (e == ESPIPE || e == EINVAL) ? ESPIPE : e;
		return -1;
	}
	ssize_t n = writing ? write(fd, buf, count) : read(fd, buf, count);
	int e = errno;
	lseek(fd, here, SEEK_SET);
	pthread_mutex_unlock(&positioned);
	errno = e;
	return n;
}

ssize_t pread(int fd, void *buf, size_t count, off_t offset)
{
	return positioned_io(fd, buf, count, offset, 0);
}

ssize_t pwrite(int fd, const void *buf, size_t count, off_t offset)
{
	return positioned_io(fd, (void *)buf, count, offset, 1);
}

int usleep(unsigned useconds)
{
	struct timespec ts = { (time_t)(useconds / 1000000u), (long)(useconds % 1000000u) * 1000L };
	return nanosleep(&ts, NULL);
}

char *mkdtemp(char *template)
{
	size_t len = strlen(template);
	if (len < 6 || memcmp(template + len - 6, "XXXXXX", 6) != 0) {
		errno = EINVAL;
		return NULL;
	}
	static const char letters[] = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
	for (int attempt = 0; attempt < 100; attempt++) {
		unsigned char r[6];
		if (getrandom(r, sizeof r, 0) != (ssize_t)sizeof r)
			return NULL;
		for (int i = 0; i < 6; i++)
			template[len - 6 + i] = letters[r[i] % (sizeof letters - 1)];
		if (mkdir(template, 0700) == 0)
			return template;
		if (errno != EEXIST)
			return NULL;
	}
	errno = EEXIST;
	return NULL;
}

const struct in6_addr in6addr_any = IN6ADDR_ANY_INIT;

int bind(int fd, const struct sockaddr *addr, socklen_t len)
{
	(void)fd; (void)addr; (void)len;
	errno = EOPNOTSUPP;
	return -1;
}

int listen(int fd, int backlog)
{
	(void)fd; (void)backlog;
	errno = EOPNOTSUPP;
	return -1;
}

int accept(int fd, struct sockaddr *addr, socklen_t *len)
{
	(void)fd; (void)addr; (void)len;
	errno = EOPNOTSUPP;
	return -1;
}

int __libc_current_sigrtmin(void)
{
	return 35;
}
