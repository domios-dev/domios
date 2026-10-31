import fcntl
import sys


with open(sys.argv[1], 'a+b') as lock:
    try:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except BlockingIOError:
        print('DEV launch owner unavailable: another launcher owns this instance', file=sys.stderr)
        sys.exit(75)
    print('locked', flush=True)
    sys.stdin.buffer.read()
