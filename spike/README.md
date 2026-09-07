# Phase 0 spike

Throwaway. Exists only to answer two questions before committing to the stack:

1. Can we play audio with sample-accurate region + loop? **Yes.**
2. Can we drag a file out of a Tauri window into another app on Wayland? **Untested - needs a human.**

## Run

```
./run.sh
```

## Test checklist

- [ ] "Pick folder..." -> choose a sound directory, files list
- [ ] Click a row -> waveform draws, metadata shows, playback starts
- [ ] Stereo file draws two lanes, mono draws one
- [ ] Drag on the waveform -> region highlights, plays that region on release
- [ ] `loop` checkbox -> region repeats seamlessly
- [ ] Space replays, arrow keys move selection
- [ ] **Drag a row from the list into DaVinci Resolve / Ardour / a file manager**

The last one is the whole point.
