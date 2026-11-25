"""Runs a shell command on a pseudo-terminal for the terminal acceptance tests.

Usage: pty_driver.py SHELL_COMMAND [MARKER=TEXT ...]

The command runs under `sh -c` as the session leader of a new pseudo-terminal, so it has a
controlling terminal. Whenever MARKER first appears in the terminal output, TEXT (with Python
escapes such as \\n and \\x03) is typed. The terminal output is copied to stdout, followed by a
final `DRIVER-EXIT <code>` line carrying the exit code of the command.
"""

import codecs
import os
import pty
import select
import signal
import sys
import time

DEADLINE_SECONDS = 90


def main():
    command = sys.argv[1]
    pending = [
        (marker, codecs.decode(text, "unicode_escape").encode())
        for marker, _, text in (arg.partition("=") for arg in sys.argv[2:])
    ]
    pid, master = pty.fork()
    if pid == 0:
        os.execvp("sh", ["sh", "-c", command])

    seen = b""
    status = None
    start = time.time()
    while time.time() - start < DEADLINE_SECONDS:
        ready, _, _ = select.select([master], [], [], 0.1)
        if ready:
            try:
                chunk = os.read(master, 4096)
            except OSError:
                chunk = b""
            if not chunk:
                break
            seen += chunk
            sys.stdout.buffer.write(chunk)
            sys.stdout.buffer.flush()
            for entry in list(pending):
                if entry[0].encode() in seen:
                    os.write(master, entry[1])
                    pending.remove(entry)
    timed_out = time.time() - start >= DEADLINE_SECONDS
    if timed_out:
        os.kill(pid, signal.SIGKILL)
    try:
        _, raw = os.waitpid(pid, 0)
        status = -9 if timed_out else os.waitstatus_to_exitcode(raw)
    except ChildProcessError:
        status = -1
    print("\nDRIVER-EXIT %d" % status)


main()
