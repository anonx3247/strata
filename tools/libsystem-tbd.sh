#!/bin/sh
# Write compiler/c/libSystem.tbd: the stub of macOS's libSystem that
# executables are linked against (lld reads it in place of the SDK's, so that
# `strata build` works on a Mac without Xcode or its command-line tools). A
# TAPI text file listing what the runtime, core.rt, std and compiled programs
# call of libSystem: the C library, libm, pthreads, dyld. At run time dyld
# finds these in the real /usr/lib/libSystem.B.dylib.
#
#   tools/libsystem-tbd.sh OBJECT... > compiler/c/libSystem.tbd
#
# The symbols are what the objects (runtime.c's, core.rt's, std's) leave
# undefined, and the list below (what programs' code may call, and what a
# linker needs of libSystem). tools/release.sh checks that the objects it
# ships need nothing else.
set -e
extra="dyld_stub_binder ___bzero ___stack_chk_fail ___stack_chk_guard ___chkstk_darwin __tlv_bootstrap
  _sin _cos _tan _asin _acos _atan _atan2 _sinh _cosh _tanh _exp _exp2 _expm1 _log _log2 _log10 _log1p
  _pow _cbrt _hypot _fmod _remainder _round _trunc _ceil _floor _sqrt _fabs _fmin _fmax _frexp _ldexp
  _modf _nan _copysign _lround _llround _rint _nearbyint _fma
  _memcpy _memmove _memset _memcmp _memchr _strlen _strcmp _strncmp _strchr _strrchr _strstr _strdup
  _strndup _strerror _strerror_r _strtod _strtol _strtoll _strtoul _strtoull _atoi _snprintf _sprintf _printf
  _fprintf _puts _putchar _fputs _fputc _fwrite _fread _fflush _fopen _fclose _getc _getline
  _malloc _calloc _realloc _free _malloc_size _posix_memalign _abort _exit __exit _atexit _qsort
  _getenv _setenv _unsetenv _environ _open _close _read _write _lseek _stat _fstat _lstat _unlink
  _rename _mkdir _rmdir _opendir _readdir _closedir _getcwd _chdir _realpath\$DARWIN_EXTSN _isatty
  _getpid _kill _raise _signal _sigaction _pipe _poll _dup _dup2 _waitpid _usleep _nanosleep
  _clock_gettime _gettimeofday _time _mmap _munmap _sysconf ___error ___stderrp ___stdinp ___stdoutp
  _pthread_create _pthread_join _pthread_self _pthread_mutex_lock _pthread_mutex_unlock
  _pthread_mutex_init _pthread_mutex_destroy _pthread_cond_wait _pthread_cond_signal
  _pthread_cond_broadcast _pthread_cond_timedwait _pthread_cond_init _pthread_cond_destroy"
defined=$(for o in "$@"; do nm -g -U "$o" | awk '{print $3}'; done | sort -u)
symbols=$( { for o in "$@"; do nm -u "$o"; done; for s in $extra; do echo "$s"; done; } \
  | grep -v '^_strata_' | sort -u | comm -23 - "$(f=$(mktemp); echo "$defined" > "$f"; echo "$f")")
cat <<EOF
--- !tapi-tbd
# libSystem, as far as Strata's executables use it: written by tools/libsystem-tbd.sh
tbd-version:     4
targets:         [ arm64-macos ]
install-name:    '/usr/lib/libSystem.B.dylib'
current-version: 1356
exports:
  - targets:     [ arm64-macos ]
    symbols:     [ $(echo "$symbols" | sed "s/.*/'&'/" | paste -sd, - | sed 's/,/, /g') ]
...
EOF
