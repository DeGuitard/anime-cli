# anime-cl 
CLI to find, download and stream anime.

## Usage
```
Usage: anime-cli -q QUERY [-e NUMBER] [-h]

Options:                               
-q, --query QUERY     Query to run
-e, --episode NUMBER  Episode number
-h, --help            print this help menu
```

#### Examples:
```
$ anime-cli -q "steins gate 0" -e 1
[HorribleSubs] Steins Gate 0 - 01 [720p].mkv": 2.23 MB / 322.02 MB [>--------] 0.69 % 1.05 MB/s 5m
```
```
$ anime-cli -q "unkown anime" -e 14
Could not find any result for this query.
```

## Pre-requisites
In order to play videos you will need mpv.

### Archlinux
```
# pacman -S mpv
```

### Debian-based
```
# apt-get install libmpv1
```

### Windows
libmpv can be found [here](https://mpv.srsfckn.biz/) for windows. You need to copy the library into your rust binaries folder for it to be linked properly.

## Disclaimer
When downloading anime, users are subject to country-specific software distribution laws. anime-dl is not designed to enable illegal activity. We do not promote piracy nor do we allow it under any circumstances. You should own an original copy of every content downloaded through this tool. Please take the time to review copyright and video distribution laws and/or policies for your country before proceeding.
