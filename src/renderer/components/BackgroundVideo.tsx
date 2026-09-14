import { useState } from 'react';
import { markVideoUnavailable } from '../background';

/**
 * The looping background video, used by both the sign-in screen and the app shell.
 *
 * The video IS the background — nothing is drawn over it, so what is in the file is what is seen.
 * It fades in only once the first frame is decodable, so the plain background underneath covers
 * the gap instead of the screen flashing black. A file that fails to load simply never appears.
 */
export function BackgroundVideo({ url, className = '' }: { url: string; className?: string }) {
  const [ready, setReady] = useState(false);

  return (
    <video
      // Keyed by url so replacing the video in Settings remounts the element; React would
      // otherwise reuse it and Chromium would keep playing the already-buffered file.
      key={url}
      className={`auth-video ${ready ? 'is-ready' : ''} ${className}`}
      src={url}
      autoPlay
      muted
      loop
      playsInline
      preload="auto"
      onCanPlay={() => setReady(true)}
      // Missing file (the protocol answers 404), unsupported codec, corrupt data — all arrive
      // here. Reporting it makes every consumer fall back to the plain background and drop the
      // toggle, instead of leaving a button that appears to do nothing.
      onError={() => markVideoUnavailable(url)}
      aria-hidden
    />
  );
}
