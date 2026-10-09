# Third-party notices

Simple MKV Player itself is licensed under the MIT License.

Binary releases include separate third-party runtime components under their own
licenses.

## FFmpeg / ffprobe

Simple MKV Player uses `ffmpeg.exe` and `ffprobe.exe` as separate programs for
audio decoding, stream inspection, and export.

The bundled FFmpeg build currently identifies itself as:

```text
ffmpeg version 2023-03-02-git-814178f926-full_build-www.gyan.dev
--enable-gpl
--enable-version3
--enable-static
```

This is a GPLv3 FFmpeg build distributed by Gyan.

FFmpeg:
https://ffmpeg.org/

Gyan Windows builds:
https://www.gyan.dev/ffmpeg/builds/

The GPLv3 license text is included with the release at:

```text
licenses\GPL-3.0.txt
```

FFmpeg and ffprobe are not covered by the Simple MKV Player MIT License.

## libmpv

Simple MKV Player dynamically loads `libmpv-2.dll` for video playback and
rendering.

The bundled DLL is an LGPL-intended libmpv build from the `zhongfly/mpv-winbuild`
project:

```text
mpv-dev-lgpl-x86_64-20261004-git-413ff0b1cd
mpv commit: 413ff0b1cd4585294803308a1a14be2fad30cede
```

mpv:
https://mpv.io/

mpv licensing information:
https://github.com/mpv-player/mpv/blob/master/Copyright

The LGPL license texts included with the release are:

```text
licenses\LGPL-2.1.txt
licenses\LGPL-3.0.txt
```

libmpv and its dependencies are not covered by the Simple MKV Player MIT License.

## Release manifest

`DEPENDENCY_MANIFEST.txt` contains SHA-256 hashes for the executable and runtime
files included in the release.

Simple MKV Player does not claim ownership of FFmpeg, ffprobe, mpv, or their
dependencies.
