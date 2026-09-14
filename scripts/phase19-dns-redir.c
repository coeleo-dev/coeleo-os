#define _GNU_SOURCE
#include <dlfcn.h>
#include <netinet/in.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>

/* Redirect QEMU/libslirp UDP/53 to the phase-19 stub (COELEO_DNS_PORT). */

static uint16_t stub_port(void)
{
    static uint16_t port;
    static int once;
    if (!once) {
        const char *e = getenv("COELEO_DNS_PORT");
        port = e ? (uint16_t)atoi(e) : 0;
        once = 1;
    }
    return port;
}

ssize_t sendto(int sockfd, const void *buf, size_t len, int flags,
               const struct sockaddr *dest_addr, socklen_t addrlen)
{
    static ssize_t (*real)(int, const void *, size_t, int, const struct sockaddr *,
                           socklen_t);
    uint16_t port = stub_port();
    struct sockaddr_in copy;

    if (!real) {
        real = dlsym(RTLD_NEXT, "sendto");
    }
    if (port && dest_addr && dest_addr->sa_family == AF_INET &&
        addrlen >= (socklen_t)sizeof(copy)) {
        memcpy(&copy, dest_addr, sizeof(copy));
        if (copy.sin_port == htons(53)) {
            copy.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
            copy.sin_port = htons(port);
            dest_addr = (const struct sockaddr *)&copy;
            addrlen = sizeof(copy);
        }
    }
    return real(sockfd, buf, len, flags, dest_addr, addrlen);
}

ssize_t recvfrom(int sockfd, void *buf, size_t len, int flags,
                 struct sockaddr *src_addr, socklen_t *addrlen)
{
    static ssize_t (*real)(int, void *, size_t, int, struct sockaddr *, socklen_t *);
    uint16_t port = stub_port();
    ssize_t n;

    if (!real) {
        real = dlsym(RTLD_NEXT, "recvfrom");
    }
    n = real(sockfd, buf, len, flags, src_addr, addrlen);
    if (n >= 0 && port && src_addr && src_addr->sa_family == AF_INET) {
        struct sockaddr_in *in = (struct sockaddr_in *)src_addr;
        if (in->sin_port == htons(port) &&
            in->sin_addr.s_addr == htonl(INADDR_LOOPBACK)) {
            in->sin_port = htons(53);
        }
    }
    return n;
}
