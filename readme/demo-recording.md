# Automated Pi demo recording

This Windows-first workflow drives a real `pie` session with `tui-test` and exports an H.264 MP4 using FFmpeg. It shows a working answer and tool Preview, Reading, a saved model default effort, a letter-marked temporary model route, and native fork/resume ancestry. The recording uses an isolated synthetic Rust workspace and native session directory; it does not replay model output.

From the repository root, build `pie` in release mode and install `tui-test`, FFmpeg/FFprobe, Node.js, and the official Pi runtime on `PATH`:

```powershell
cargo build --release -p e-pi --bin pie
node tools/record-demo.mjs --pie target/release/pie.exe --model codemaker/deepseek-flash --output demo.mp4 --preflight
node tools/record-demo.mjs --pie target/release/pie.exe --model codemaker/deepseek-flash --output demo.mp4
```

Replace `--model` with an **exact** provider/model entry in the local Pi catalog. The script checks this identity and its supported reasoning efforts before opening the TUI. `--compare-model provider/model` optionally chooses a different verified resting route; by default it prefers another available model. `--preflight` checks tools and routes without generating. The live run makes two model requests at most in the current scenario; it has a four-call safety cap, a three-minute deadline per request, and no automatic retries. Model calls may incur cost. Do not run another `pie` or `dshe` against the same frontend settings concurrently.

The driver temporarily uses the existing frontend config to demonstrate model marks and default effort. It backs up config and Pi resume state and restores its own changes on exit. If another writer changes either file, it leaves that change intact and reports the backup directory for manual review. Native sessions and config backups stay in the current user's temp directory; the synthetic workspace uses the Windows Public directory so the recorded footer does not show a personal home path. Only the script-owned temporary workspace and processes are cleaned up. Pi authentication remains in its native store.

A successful run writes `demo.mp4` and `demo.mp4.json` (duration, pacing, routes, and edit disclosure). It does not overwrite an existing video. Commands and prompts are typed one character at a time, with at least 100 ms between characters. The completed input remains visible for one second before Enter; menu/navigation keys also have a one-second gap. Model output is not slowed. The current exporter preserves every recorded frame at normal speed. The 90-second target is temporarily advisory during pacing review; longer videos are reported in the sidecar rather than rejected. Trimming slow generations without hiding interactions remains to be implemented. `tui-test` renders terminal output, not the exact pixels or decorations of Windows Terminal. Inspect every scene and the footer for private information before sharing a video. No subtitles or audio are added.
