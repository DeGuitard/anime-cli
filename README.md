# anime-cli
Command Line Interface to find, download, and stream anime.

## Usage
```
Usage: anime-cli -q "QUERY" [-e NUMBER] [-b NUMBER] [-r NUMBER] [-n] [-h]

Options:                               
-q, --query         QUERY   Query to run
-e, --episode       NUMBER  Episode number
-b, --batch         NUMBER  Download episodes from -e up to -b
-r, --resolution    NUMBER  Specifies resolution, default is 720, put 0 in order to remove resolution from search
-n, --noshow                Do not automatically open media player
-h, --help                  Print this help menu
```

Calling `anime-cli` without any flags and arguments or double clicking the executable (if you're on Windows) will cause it to prompt the user for inputs.

#### Examples:
```
$ anime-cli -q "steins gate 0" -e 1
[HorribleSubs] Steins Gate 0 - 01 [720p].mkv": 2.23 MB / 322.02 MB [>--------] 0.69 % 1.05 MB/s 5m
```
```
$ anime-cli -q "unknown anime" -e 14
Could not find any result for this query.
```
```
$ anime-cli -q "Sakamoto Desu ga" -b 12
[HorribleSubs] Sakamoto desu ga - 01 [720p].mkv: 329.73 MB / 329.73 MB [==========================] 100.00 % 4.69 MB/s
[HorribleSubs] Sakamoto desu ga - 02 [720p].mkv: 329.07 MB / 329.07 MB [==========================] 100.00 % 4.95 MB/s
[HorribleSubs] Sakamoto desu ga - 03 [720p].mkv: 215.03 MB / 329.65 MB [================>---------]  65.23 % 4.23 MB/s
[HorribleSubs] Sakamoto desu ga - 04 [720p].mkv: 144.81 MB / 329.33 MB [===========>--------------]  43.97 % 4.10 MB/s
...
```

If you've chosen to use MPV, as soon as you start downloading, MPV will open and begin playing the media file. If you're downloading multiple files at once, it will automatically move onto the next movie when the previous movie ends.

## Pre-requisites
In order to play videos you will need mpv.

However, if you do not wish to autoplay videos, you can do
`cargo build --release --no-default-features`

### Archlinux
```
# pacman -S mpv
```

### Debian-based
```
# apt-get install libmpv1
```

### Windows
libmpv can be found [here](https://mpv.srsfckn.biz/) for windows. Click on \[Dev]
Extract files to any location.
Copy from x86_64 for 64bit or i686 for 32bit the following files.

libmpv.dll.a -> $(project)/target/debug/deps/       and rename to mpv.lib

mpv-1.dll    -> where `anime-cli.exe` is

## Disclaimer
When downloading anime, users are subject to country-specific software distribution laws. anime-cli is not designed to enable illegal activity. We do not promote piracy nor do we allow it under any circumstances. You should own an original copy of every content downloaded through this tool. Please take the time to review copyright and video distribution laws and/or policies for your country before proceeding.

## Todo
* Support more media viewers such as VLC (if you compile without mpv it'll open in your default media player)
* Make this work on android (You can install a linux environment and use this tool to just download)
* A search only function as well as select correct anime from list
* Make an anime log so you can resume where you left off
* Add a graphical interface (opens a terminal with inputs, close enough)