"use client";
export default function UsersError({ reset }: { reset: () => void }) {
  return <><p role="alert">Could not complete the request. Please refresh and try again.</p><button onClick={reset}>Try again</button></>;
}
