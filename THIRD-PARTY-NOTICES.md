# Third-party notices

Anix uses open-source components. Components under licences that ask for a notice are listed here.

## anitomy-ng

- Used for: reading episode numbers, titles and release details from video file names.
- Licence: Mozilla Public License 2.0 (https://www.mozilla.org/en-US/MPL/2.0/).
- Used unmodified. Source code: https://crates.io/crates/anitomy-ng (version 1.0.10).

## libmpv (mpv)

- Used for: the embedded video player (Settings › Playback › Player engine), loaded at run time.
- Licence: GNU Lesser General Public License 2.1 or later (https://www.gnu.org/licenses/old-licenses/lgpl-2.1.html). Built with `-Dgpl=false`, so only its LGPL parts are included.
- Used unmodified and dynamically linked (`lib/anix/libmpv.so.2` in the AppImage; the system's `libmpv.so.2` in the RPM and DEB packages). You may replace it with your own build of the same interface. Source code: https://github.com/mpv-player/mpv (version 0.41.0); build script: `scripts/build-libmpv-lgpl.sh`.

## FFmpeg

- Used for: decoding video and audio inside libmpv.
- Licence: GNU Lesser General Public License 2.1 or later. Configured without `--enable-gpl` and `--enable-nonfree`; the AppImage build refuses any other build.
- Used unmodified and dynamically linked (`lib/anix/libav*.so`, `libsw*.so`). Source code: https://ffmpeg.org/releases/ffmpeg-8.0.1.tar.xz.
- The web player's GStreamer libav plugin gets an LGPL FFmpeg too. When the build system's plugin is linked against an older FFmpeg series, that release is built with the same options and bundled next to it (on the Ubuntu 22.04 release build: FFmpeg 4.4.6, https://ffmpeg.org/releases/ffmpeg-4.4.6.tar.xz). The LGPL texts of FFmpeg, mpv and libplacebo are in `usr/share/licenses/anix/` in the AppImage.

## nv-codec-headers

- Used for: NVDEC hardware decoding on NVIDIA cards inside FFmpeg. Headers only: FFmpeg opens the NVIDIA driver's own libraries at run time, and nothing from NVIDIA is bundled.
- Licence: MIT (the notice heads each header; a copy is in `usr/share/licenses/anix/nv-codec-headers-MIT.txt` in the AppImage). Used unmodified. Source code: https://github.com/FFmpeg/nv-codec-headers (n12.1.14.0).

## libplacebo

- Used for: drawing the video with OpenGL inside libmpv.
- Licence: GNU Lesser General Public License 2.1 or later. Used unmodified and dynamically linked. Source code: https://code.videolan.org/videolan/libplacebo (v7.351.0).

## libass

- Used for: drawing styled (ASS) subtitles inside libmpv.
- Licence: ISC, copyright the libass authors (full text in `usr/share/licenses/anix/libass-ISC.txt` in the AppImage). Used unmodified and dynamically linked. Source code: https://github.com/libass/libass (0.17.4).

## Anime4K

- Used for: the optional Anime4K upscaling presets of the embedded player (`resources/anime4k`).
- Licence: MIT License. Copyright (c) 2019-2021 bloc97. The full licence text is in `resources/anime4k/LICENSE` and at the top of each shader file.
- The GLSL shader files of release v4.0.1 are included unmodified. Source: https://github.com/bloc97/Anime4K.

## dav1d, libunwind, Little CMS, libjpeg-turbo

- Used for: libraries the embedded player's libmpv/FFmpeg build links against, bundled in the AppImage (`usr/lib/anix`) for systems that lack these versions. Used unmodified and dynamically linked.
- dav1d (`libdav1d.so.7`, AV1 decoding): BSD 2-Clause License, copyright VideoLAN and dav1d authors. Source: https://code.videolan.org/videolan/dav1d
- libunwind (`libunwind.so.8`): MIT License. Source: https://github.com/libunwind/libunwind
- Little CMS (`liblcms2.so.2`, colour management): MIT License (the library; only the library is bundled). Source: https://github.com/mm2/Little-CMS
- libjpeg-turbo (`libjpeg.so.62`): IJG License, modified BSD 3-Clause License and zlib License. This software is based in part on the work of the Independent JPEG Group. Source: https://github.com/libjpeg-turbo/libjpeg-turbo
- libdisplay-info (`libdisplay-info.so`, display information for mpv's DRM and VA-API support; bundled only when the build system lacks it): MIT License. Source: https://gitlab.freedesktop.org/emersion/libdisplay-info

## Meme images (Meme mode › High)

- Used for: the pictures shown only when Settings › Appearance › Meme mode is High. They ship as one pack, `assets/memes.zip`, unpacked into the app at build time.
- **Not covered by the MIT License.** The images belong to their rights holders:

| Series | Characters | Rights holders |
|---|---|---|
| Dragon Ball Z / Dragon Ball Super | Goku, Vegeta, Frieza, Yamcha | Bird Studio/Shueisha, Toei Animation |
| Naruto | Naruto, Kakashi | Masashi Kishimoto/Shueisha, Pierrot |
| One Piece | Luffy, Zoro | Eiichiro Oda/Shueisha, Toei Animation |
| JoJo’s Bizarre Adventure | Dio, Jotaro, the ‘To Be Continued’ plate | LUCKY LAND COMMUNICATIONS/Shueisha, JoJo’s Animation Project |
| Fist of the North Star | Kenshiro | Buronson & Tetsuo Hara/Coamix, Toei Animation |
| Death Note | Light | Tsugumi Ohba, Takeshi Obata/Shueisha, VAP, Madhouse |
| Steins;Gate | Mayuri, Okabe | MAGES./Chiyo St. Inc., White Fox |
| Fate/stay night, Unlimited Blade Works, Fate/Zero | Saber, Gilgamesh, Archer, Shirou, Rin, Lancer, Kirei, Iskandar | TYPE-MOON (anime: ufotable; 2006 series: Studio DEEN) |
| Fate/Grand Order | Mash | TYPE-MOON / FGO PROJECT |
| Pokémon | Pikachu | Nintendo, Creatures, GAME FREAK, TV Tokyo, ShoPro, JR Kikaku |
| The Brave Fighter of Sun Fighbird | the android (‘Is this a pigeon?’) | Sunrise |
| Demon Slayer | Zenitsu | Koyoharu Gotōge/Shueisha, Aniplex, ufotable |
| Spy x Family | Anya | Tatsuya Endo/Shueisha, Wit Studio × CloverWorks |
| Bocchi the Rock! | Hitori Gotoh | Aki Hamaji/Houbunsha, CloverWorks |
| Solo Leveling | Sung Jinwoo | Chugong, DUBU (REDICE Studio)/D&C Media, A-1 Pictures |
| Gurren Lagann | Kamina | GAINAX, Kazuki Nakashima/Aniplex |

- Fate/stay night: Unlimited Blade Works Abridged lines: Project Mouthwash (fan parody series).
- Sources: Know Your Meme, Tenor and each series’ fan wiki. Each image’s source address and credit are in `memes/manifest.json` inside `assets/memes.zip`.
- Series quoted in meme lines only, with no image: Bleach, Attack on Titan, One Punch Man, Jujutsu Kaisen, Sailor Moon, Detective Conan, Frieren, Dandadan, Mob Psycho 100, Chainsaw Man, Cowboy Bebop, Evangelion, Hunter x Hunter, Fullmetal Alchemist, Kaguya-sama, Komi Can’t Communicate, Monogatari, Haruhi Suzumiya, The Eminence in Shadow.
