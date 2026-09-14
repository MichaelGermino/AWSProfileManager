# Renderer assets

## `auth-background.mp4` (optional)

Drop a video here named `auth-background.mp4` (or `.webm`) and the master-password screen uses it
as its background automatically. Remove it and the CSS "silk" background is used instead. No code
change either way — see `components/AuthVideoBackground.tsx`.

Two things to weigh before adding one:

- **Licensing.** The file is bundled into the NSIS installer and redistributed to every user. Use
  footage you own or that carries a permissive licence (Pexels, Pixabay, Coverr and similar allow
  commercial redistribution). A clip downloaded from a video site is not automatically usable just
  because it is publicly viewable.
- **Size.** `nsis.differentialPackage` is `false` (see CLAUDE.md), so electron-updater downloads
  the entire app on every update. Every megabyte here is a megabyte added to every future update
  for every user. Prefer a short, seamless, well-compressed loop:

  ```bash
  ffmpeg -i source.mp4 -t 12 -vf "scale=1280:-2,fps=30" -c:v libx264 -crf 30 -preset slow \
         -profile:v high -pix_fmt yuv420p -an -movflags +faststart auth-background.mp4
  ```

  `-an` drops the audio track — it is muted anyway, so shipping it is pure waste.
