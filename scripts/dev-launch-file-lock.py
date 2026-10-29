import fcntl
import sys


with open(sys.argv[1], 'a+b') as lock:
    try:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except BlockingIOError:
        sys.exit('DEV launch owner unavailable: another launcher owns this instance')
    print('locked', flush=True)
    sys.stdin.buffer.read()
