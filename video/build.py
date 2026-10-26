#!/usr/bin/env python3
"""Assemble the demo video: shots/ PNGs + audio/ narration -> out.mp4.

Pipeline per section of script.md:
  shot PNG (or generated placeholder) -> ffmpeg zoompan segment with
  burned title chip + caption -> concat -> mux concatenated narration.

Usage:
  build.py           assemble out.mp4 (placeholders for missing shots,
                     silent audio for missing narration)
  build.py --tts     run edge-tts on every section's `voice:` first
"""
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent
SHOTS, AUDIO, SEGS = ROOT / "shots", ROOT / "audio", ROOT / "segs"
SCRIPT = ROOT / "script.md"
OUT = ROOT / "out.mp4"
VOICE = "en-US-AndrewMultilingualNeural"
W, H, FPS = 1920, 1080, 30
PAD = 0.5  # trailing silence per section, seconds

FONT = subprocess.run(
    ["fc-match", "-f", "%{file}", "DejaVuSans"],
    capture_output=True, text=True, check=True,
).stdout.strip()


def sh(*args: str) -> None:
    r = subprocess.run(args, capture_output=True, text=True)
    if r.returncode != 0:
        sys.exit(f"FAILED: {' '.join(args)}\n{r.stderr[-2000:]}")


def ffprobe_dur(p: Path) -> float:
    r = subprocess.run(
        ["ffprobe", "-v", "quiet", "-show_entries", "format=duration",
         "-of", "csv=p=0", str(p)],
        capture_output=True, text=True, check=True)
    return float(r.stdout.strip())


def esc(t: str) -> str:
    return (t.replace("\\", "\\\\").replace(":", "\\:")
             .replace("'", "\u2019").replace("%", "\\%")
             .replace("\n", " ").strip())


def parse_script() -> list[dict]:
    secs = []
    for m in re.finditer(r"^## (.+?)\n(.*?)(?=^## |\Z)",
                         SCRIPT.read_text(), re.M | re.S):
        head, body = m.group(1).strip(), m.group(2)
        sec = {"title": head.split(". ", 1)[-1]}
        voice_start = body.find("voice:")
        fields, voice = (body[:voice_start],
                         body[voice_start + 6:]) if voice_start >= 0 \
            else (body, "")
        for k, v in re.findall(r"^(\w+):\s*(.+)$", fields, re.M):
            sec[k] = v.strip()
        sec["voice"] = " ".join(voice.split())
        secs.append(sec)
    return secs


def gen_audio(secs: list[dict]) -> None:
    edge = ROOT.parents[1] / ".venv/bin/edge-tts"
    for i, s in enumerate(secs):
        mp3 = AUDIO / f"{i + 1:02d}.mp3"
        if s["voice"] and not mp3.exists():
            sh(str(edge), "--voice", VOICE, "--text", s["voice"],
               "--write-media", str(mp3))
            print(f"audio/{mp3.name}")


def gen_placeholder(sec_num: str, title: str) -> Path:
    png = SHOTS / f"ph_{sec_num}.png"
    if not png.exists():
        sh("ffmpeg", "-y", "-f", "lavfi",
           "-i", f"color=c=0x0d1117:s={W}x{H}:d=1", "-frames:v", "1",
           "-vf", (f"drawtext=fontfile={FONT}:text='{esc(title)}'"
                   f":fontsize=72:fontcolor=white:x=(w-text_w)/2:y=(h-text_h)/2-40,"
                   f"drawtext=fontfile={FONT}:text='[ screenshot placeholder ]'"
                   f":fontsize=32:fontcolor=0x8b949e:x=(w-text_w)/2:y=(h-text_h)/2+60"),
           str(png))
    return png


def main() -> None:
    secs = parse_script()
    for d in (SHOTS, AUDIO, SEGS):
        d.mkdir(exist_ok=True)
    if "--tts" in sys.argv:
        gen_audio(secs)

    segs, wavs = [], []
    for i, s in enumerate(secs):
        n = f"{i + 1:02d}"
        mp3 = AUDIO / f"{n}.mp3"
        dur = ffprobe_dur(mp3) + PAD if mp3.exists() else float(s.get("dur", 8))
        shot = SHOTS / s.get("shot", "")
        png = shot if shot.exists() else gen_placeholder(n, s["title"])

        frames = int(dur * FPS)
        seg = SEGS / f"{n}.mp4"
        vf = (
            f"scale={W}:{H}:force_original_aspect_ratio=increase,"
            f"crop={W}:{H},"
            f"zoompan=z='min(1+0.0015*on,1.15)':"
            f"x='iw/2-(iw/zoom/2)':y='ih/2-(ih/zoom/2)':"
            f"d={frames}:s={W}x{H}:fps={FPS},"
            f"drawtext=fontfile={FONT}:text='{esc(s['title'])}'"
            f":fontsize=40:fontcolor=white:x=60:y=40:"
            "box=1:boxcolor=black@0.55:boxborderw=18,"
            f"drawbox=x=0:y=h-110:w=iw:h=110:color=black@0.55:t=fill,"
            f"drawtext=fontfile={FONT}:text='{esc(s.get('caption', ''))}'"
            f":fontsize=44:fontcolor=white:x=(w-text_w)/2:y=h-78,"
            "format=yuv420p"
        )
        sh("ffmpeg", "-y", "-loop", "1", "-i", str(png),
           "-vf", vf, "-frames:v", str(frames), "-r", str(FPS),
           "-c:v", "libx264", "-preset", "fast", "-crf", "20", str(seg))
        segs.append(seg)

        wav = SEGS / f"{n}.wav"
        if mp3.exists():
            sh("ffmpeg", "-y", "-i", str(mp3), "-af", f"apad,atrim=0:{dur}",
               "-ar", "44100", "-ac", "2", str(wav))
        else:
            sh("ffmpeg", "-y", "-f", "lavfi", "-i", "anullsrc",
               "-t", str(dur), "-ar", "44100", "-ac", "2", str(wav))
        wavs.append(wav)
        print(f"seg {n}  {dur:5.1f}s  {png.name}")

    lst = SEGS / "concat.txt"
    lst.write_text("".join(f"file '{p.name}'\n" for p in segs))
    video = SEGS / "video.mp4"
    sh("ffmpeg", "-y", "-f", "concat", "-safe", "0", "-i", str(lst),
       "-c", "copy", str(video))

    nlst = SEGS / "nconcat.txt"
    nlst.write_text("".join(f"file '{p.name}'\n" for p in wavs))
    narr = SEGS / "narr.wav"
    sh("ffmpeg", "-y", "-f", "concat", "-safe", "0", "-i", str(nlst),
       "-c:a", "pcm_s16le", str(narr))

    sh("ffmpeg", "-y", "-i", str(video), "-i", str(narr),
       "-c:v", "copy", "-c:a", "aac", "-b:a", "160k", "-shortest", str(OUT))
    print(f"\n{OUT}  {ffprobe_dur(OUT):.1f}s")


if __name__ == "__main__":
    main()
