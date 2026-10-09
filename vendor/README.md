# Runtime binaries

Release builds package the exact files currently placed here:

```text
vendor\ffmpeg\ffmpeg.exe
vendor\ffmpeg\ffprobe.exe
vendor\mpv\libmpv-2.dll
```

Use:

```powershell
.\scripts\stage-local-runtime.ps1
```

to copy the current FFmpeg and ffprobe from PATH into the vendor folder.

The large runtime binaries are ignored by Git.
