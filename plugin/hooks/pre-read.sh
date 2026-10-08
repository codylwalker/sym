#!/bin/sh
# PreToolUse(Read): if the `sym` binary is installed, let it look at the
# file about to be read and, for a big source file, add a hint pointing at
# the skeleton. Silent (and exit 0) when sym is missing or has nothing to say.
command -v sym >/dev/null 2>&1 || exit 0
exec sym hook pre
