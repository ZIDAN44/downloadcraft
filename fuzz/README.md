# Fuzzing

Fuzz targets for the code that reads what DownloadCraft doesn't control: servers' headers, links,
yt-dlp's answers, GitHub's release list and the files left on disk. This is its own Cargo
workspace, outside the main one, so the main build never needs nightly or libFuzzer.
`.github/workflows/fuzz.yml` runs every target weekly on Linux. From MailCraft's `fuzz/`.

| Target | What it reads |
|---|---|
| `names` | `Content-Disposition`, links and file names: whatever comes in, the name is safe as one file name on every system, and making it safe twice changes nothing |
| `http` | `Content-Range`; the parts record beside a partial file (one let through covers the file exactly once); the updater's release list, `SHA256SUMS.txt`, tags and asset links |
| `video` | yt-dlp's `-J` description of a page (qualities, title, file name) and its progress lines |
| `saved_list` | `downloads.json`: an error or a list, and a list loaded saves and loads again the same |

## Running

Linux or macOS (or WSL), with a nightly toolchain and `cargo install cargo-fuzz`:

```bash
cd fuzz
cargo +nightly fuzz run -O names corpus/names seeds/names -- -dict=dicts/names.dict -max_total_time=300
```

`seeds/` holds a hand-written valid input per target and `dicts/` the format's keywords, to get the
fuzzer going quickly; `corpus/` and `artifacts/` stay out of git. A crash leaves its input in
`artifacts/<target>/`: replay it with `cargo +nightly fuzz run -O <target> <file>`, fix the code,
and add the input to a regression test next to the code (never only to the corpus).
