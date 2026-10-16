import { watch, statSync, openSync, readSync, closeSync } from 'node:fs';
import { basename, dirname } from 'node:path';

export function scanLogAck(carry, chunk, marker) {
  const text = carry + chunk;
  return { matched: text.includes(marker), carry: text.slice(-(marker.length - 1)) };
}

export function fileLogSource(path, maxBytes = 8 * 1024 * 1024) {
  return {
    position: () => statSync(path).size,
    subscribe(callback) {
      const observer = watch(dirname(path), (_event, name) => {
        if (!name || String(name) === basename(path)) callback();
      });
      return () => observer.close();
    },
    readFrom(position) {
      const end = statSync(path).size;
      if (end < position) throw Error('Acknowledgement log rotated');
      if (end - position > maxBytes) throw Error('Acknowledgement log window exceeded bound');
      const bytes = Buffer.alloc(end - position), fd = openSync(path, 'r');
      let read = 0;
      try {
        while (read < bytes.length) {
          const count = readSync(fd, bytes, read, bytes.length - read, position + read);
          if (!count) break;
          read += count;
        }
      } finally { closeSync(fd); }
      return { position: position + read, text: bytes.subarray(0, read).toString() };
    },
  };
}

export function observeLogMutation({ source, mutate, marker, signal }) {
  return new Promise((resolve, reject) => {
    let position = source.position(), carry = '', unsubscribe, finished = false;
    const finish = error => {
      if (finished) return;
      finished = true;
      unsubscribe?.();
      signal.removeEventListener('abort', abort);
      error ? reject(error) : resolve({ position });
    };
    const abort = () => finish(signal.reason);
    const drain = () => {
      if (finished) return;
      try {
        const next = source.readFrom(position);
        if (next.position < position) throw Error('Acknowledgement log rotated');
        position = next.position;
        const result = scanLogAck(carry, next.text, marker);
        carry = result.carry;
        if (result.matched) finish();
      } catch (error) { finish(error); }
    };
    try {
      unsubscribe = source.subscribe(drain);
      signal.addEventListener('abort', abort, { once: true });
      if (signal.aborted) return abort();
      mutate();
      drain();
    } catch (error) { finish(error); }
  });
}
