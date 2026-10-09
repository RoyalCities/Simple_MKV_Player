# Third-party runtime

The release builder expects:

```text
vendor\ffmpeg\ffmpeg.exe
vendor\ffmpeg\ffprobe.exe
vendor\mpv\libmpv-2.dll
```

For the current development machine, run:

```powershell
.\scripts\stage-local-runtime.ps1
```

This copies the known-working FFmpeg and ffprobe from PATH into `vendor\ffmpeg`.

The runtime binaries themselves are ignored by Git. Release ZIPs/installers
contain copies of them plus the appropriate third-party license notices.
