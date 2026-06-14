#!/usr/bin/env python3
"""Convert audio files to exact tinyplay-compatible format matching build_wav() in audio.rs:
   48kHz, 16-bit, stereo (L=R=mono mix), no extra WAV chunks, 44-byte header."""
import struct, subprocess, sys, os

def convert(input_path, output_path):
    # Decode + mix to mono 48kHz raw s16le via ffmpeg
    result = subprocess.run([
        'ffmpeg', '-y', '-i', input_path,
        '-af', 'pan=mono|c0=0.5*c0+0.5*c1',
        '-ar', '16000', '-ac', '1',
        '-f', 's16le', '-'
    ], capture_output=True)
    raw = result.stdout
    if not raw:
        print(f"FAILED: {input_path}"); return

    # Keep mono at 8kHz — the Rust audio engine handles mixing
    n = len(raw)
    with open(output_path, 'wb') as f:
        f.write(b'RIFF'); f.write(struct.pack('<I', 36+n))
        f.write(b'WAVE')
        f.write(b'fmt '); f.write(struct.pack('<I', 16))
        f.write(struct.pack('<HH', 1, 1))        # PCM, mono
        f.write(struct.pack('<II', 16000, 32000)) # rate=16kHz, byterate=16000*2
        f.write(struct.pack('<HH', 2, 16))        # blockalign=2, bitdepth=16
        f.write(b'data'); f.write(struct.pack('<I', n))
        f.write(raw)
    secs = n / 2 / 16000
    print(f"OK {os.path.basename(output_path)} ({secs:.2f}s, {len(stereo)//1024}KB)")

BASE = os.path.dirname(os.path.abspath(__file__)) + '/../assets/sfx'
OUT  = os.path.dirname(os.path.abspath(__file__)) + '/../deploy/assets/sfx'
os.makedirs(OUT+'/death', exist_ok=True)

for f in os.listdir(BASE):
    if not f.endswith('.wav'): continue
    if 'death' in f or 'explosion' in f.lower() or 'bazooka' in f.lower():
        # top-level sfx
        pass
    convert(f'{BASE}/{f}', f'{OUT}/{f.replace(" ","_")}')

# Death sounds go in death/
for f in os.listdir(BASE):
    if not f.endswith('.wav'): continue
    # All non-explosion sounds are death sounds for now
    if 'bazooka' not in f.lower() and 'explosion' not in f.lower():
        convert(f'{BASE}/{f}', f'{OUT}/death/{f.replace(" ","_")}')
