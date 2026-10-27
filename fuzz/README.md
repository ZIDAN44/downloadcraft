# Fuzzing

Fuzz targets for the code that reads input DownloadCraft doesn't control: servers' headers, links,
yt-dlp's answers, GitHub's release list, torrent trackers' answers, and files left on disk. This folder is a Cargo workspace
of its own, outside the main one, so the main build never needs a nightly toolchain or libFuzzer.
`.github/workflows/fuzz.yml` runs every target weekly on Linux. The set-up comes from MailCraft's
`fuzz/`.

| Target | What it reads, and what it checks |
|---|---|
| `names` | `Content-Disposition` headers, links and file names. Whatever comes in, the result is a safe single file name on every system, and sanitising it again changes nothing. |
| `http` | `Content-Range` headers; the parts record beside a partial file (a record that's accepted covers the file exactly once); the updater's release list, `SHA256SUMS.txt`, tags and asset links. |
| `video` | yt-dlp's `-J` description of a page (qualities, title, file name) and its progress lines. |
| `saved_list` | `downloads.json`. It loads as an error or a list, and a list it loads saves and loads again unchanged. |
| `torrent` | Trackers' scrape answers, bencoded (BEP 48) and over UDP (BEP 15); IP filter files in the `.dat` and P2P forms; proxy addresses; info hashes. A filter comes out as P2P ranges only, and a proxy that's taken reads the same again. |

## Running

On Linux or macOS (WSL works), with a nightly toolchain and `cargo install cargo-fuzz`:

```bash
cd fuzz
cargo +nightly fuzz run -O names corpus/names seeds/names -- -dict=dicts/names.dict -max_total_time=300
```

`seeds/` holds one hand-written valid input per target and `dicts/` the format's keywords, both to
get the fuzzer going quickly. `corpus/` and `artifacts/` stay out of Git.

## When it finds a crash

The input that caused it is saved in `artifacts/<target>/`.

1. Replay it with `cargo +nightly fuzz run -O <target> <file>`.
2. Fix the code.
3. Add the input to a regression test next to the code, never only to the corpus.
