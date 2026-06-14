# Miyoo Mini Plus — Raw ALSA Audio Notes

Lessons learned getting audio working in a Rust game targeting Onion OS on the Miyoo Mini Plus (Cortex-A7, 1.2 GHz, 128MB DDR3). Accumulated through trial and error — passing these along so you don't have to rediscover them.

---

## The core discovery: the ALSA stub lies to you

The Miyoo ships with a stub `libasound.so.2` (the MI audioserver layer). No matter what sample rate you pass to `snd_pcm_set_params`, **the DAC is always clocked at 8000 Hz mono**. If you load a 44100 Hz stereo WAV and try to play it verbatim, you'll hear chipmunk-speed garbage.

The fix: resample everything to **8000 Hz mono yourself**, before it touches ALSA. That's not ALSA's job here.

---

## Resampling

Integer-only nearest-neighbour — no floats. The Cortex-A7 has a weak FPU and you're on a real-time budget.

```
out_frame[i]  →  src_frame = (i * in_rate) / out_rate
```

For stereo → mono: average the two channels with `(L + R) >> 1`.

Do this on the thread that loads the file, not in the audio loop. By the time the buffer reaches the mixer it should already be 8000 Hz mono 16-bit PCM, stored in an `Arc<Vec<u8>>` so you can share it without copying.

---

## Don't link libasound at compile time — dlopen it

The library isn't guaranteed to be at a fixed path. Try candidates in order:

1. `/customer/lib/libasound.so.2` — the MI system location (most reliable)
2. `/mnt/SDCARD/miyoo/lib/libasound.so.2` — some Onion OS configurations
3. `/usr/lib/libasound.so.2` — fallback
4. `libasound.so.2` — ld.so search path

Compile with **no** `-lasound` link flag. If every dlopen fails, audio simply won't initialize — the rest of the game should keep running.

You only need five symbols:

```c
snd_pcm_open
snd_pcm_set_params
snd_pcm_writei
snd_pcm_close
snd_pcm_recover
```

---

## Opening the device

```c
snd_pcm_open(&pcm, "hw:0,0", SND_PCM_STREAM_PLAYBACK, 0);

snd_pcm_set_params(
    pcm,
    SND_PCM_FORMAT_S16_LE,          // format
    SND_PCM_ACCESS_RW_INTERLEAVED,  // access
    1,        // channels — MONO, always
    8000,     // rate — the stub ignores this but pass it anyway
    0,        // soft_resample — disabled
    100000    // latency hint in microseconds
);
```

The period size the driver reports is **200 frames**. Write exactly 200 frames (400 bytes) per `snd_pcm_writei` call. Writing more or fewer confuses the stub.

---

## Use a dedicated mixer thread

Don't write to ALSA from your game loop. Spawn a thread that owns the PCM handle exclusively. Your game sends commands over a bounded channel (capacity ~16 is fine).

Each period (~25ms at 8000 Hz) the mixer thread:
1. Drains the command queue non-blocking
2. Builds a 200-frame mix buffer: music (looping) + SFX slots
3. Calls `snd_pcm_writei` once

Loading and resampling happen on the *sending* thread before the buffer is queued, so the mixer thread is never blocked by I/O.

### Mixing

Keep everything in `i32` while summing, then hard-clamp to `i16` range before writing. Don't let it overflow silently.

For volume, use Q15 fixed-point — multiply the `i32` sample by a scalar and `>> 15`:

```
// ~80% volume
let vol: i32 = 26214; // 26214 / 32768 ≈ 0.80
let out_sample = (raw_sample * vol) >> 15;
```

Cap simultaneous SFX at 4 slots. Beyond that, drop new ones silently. Music loops by resetting the playback position to 0 when it reaches the end.

### Error recovery

If `snd_pcm_writei` returns a negative value, call `snd_pcm_recover(pcm, err, 1)` (the `1` means silent — it won't log) and continue. Don't close and reopen the handle — the stub doesn't handle that gracefully.

---

## Other footguns

**SDL2 audio won't work.** Even if the library is present, the `mmiyoo` SDL2 backend requires `audioserver` to be running. Most launch scripts stop it to free memory on game start. Go raw ALSA.

**OGG and MP3 won't work.** SDL_Mixer's OGG decoding is broken on this platform. WAV only.

**Only accept 16-bit PCM WAVs.** Reject and log 8-bit and 32-bit files. The resampler needs 16-bit and it's not worth handling the other cases.

**Parse the WAV header yourself.** It's ~20 lines: find the `fmt ` chunk for sample rate, channel count, and bit depth; find the `data` chunk for the PCM offset. Don't assume a fixed 44-byte header — chunks can appear in any order.

---

## WAV format requirements (summary)

| Property | Required |
|---|---|
| Encoding | 16-bit PCM (no float, no compressed) |
| Channels | Mono or stereo — both work, stereo gets averaged to mono |
| Sample rate | Any — you resample to 8000 Hz on load |
| Container | WAV/RIFF only |

---

## What we shipped

For reference, the RPG98 audio system is about 340 lines of Rust in a single file (`client/src/audio.rs`). The public API is three calls:

```rust
let mixer = AudioMixer::start();          // None if libasound missing
mixer.play_music("path/to/loop.wav");
mixer.play_sfx("path/to/hit.wav");
mixer.fade_music();                       // ~1s linear fade, then stop
mixer.stop();                             // called on exit
```

Everything else — resampling, mixing, period writes, error recovery — is internal to the mixer thread.
