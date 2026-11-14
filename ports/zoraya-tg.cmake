# CMake toolchain file of zoraya-tg: Zoraya's own (ports/cmake/zoraya.cmake of
# the Zoraya checkout named by the environment variable ZORAYA), with Zoraya's
# built ports (build/ports/deps: OpenSSL, zlib) searched as well as the
# sysroot. The environment, not a cache variable: try_compile projects read
# this file again and see the same environment.
set(ZORAYA_CHECKOUT "$ENV{ZORAYA}")
if(ZORAYA_CHECKOUT STREQUAL "")
  message(FATAL_ERROR "zoraya-tg.cmake: set ZORAYA=<Zoraya checkout> in the environment")
endif()
include("${ZORAYA_CHECKOUT}/ports/cmake/zoraya.cmake")
list(APPEND CMAKE_FIND_ROOT_PATH "${ZORAYA_CHECKOUT}/build/ports/deps")
