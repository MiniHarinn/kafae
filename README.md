<div align="center">

# (っ-,-)つ☕ x 10¹²

A terminal client for [Cafe Grader](https://github.com/cafe-grader-team/cafe-grader-web).<br>
*kafae* (กาแฟ) is Thai for coffee. Not affiliated with the Cafe Grader team nor Chulalongkorn University.<br>

</div>


## Install

Via flake:

```nix
inputs.kafae.url = "github:MiniHarinn/kafae";
```

Or try it without installing:

```bash
nix run github:MiniHarinn/kafae -- problems # Login first tho :)
```

## Usage

```console
$ kafae login                     # once per 12h
$ kafae problems                  # what you can submit to, with your best score
$ kafae view 01_Str_11            # PDF to your viewer, description to the terminal
$ kafae new 01_Str_11             # writes 01_Str_11.cpp from a template
$ kafae run 01_Str_11.cpp         # compile and run here, not on the grader
$ kafae test 01_Str_11.cpp        # run the grader's testcases locally
$ kafae submit 01_Str_11.cpp      # compile check, submit, wait for the verdict
$ kafae status 1234               # check a verdict later
```

Names come from the filename, so nothing downstream needs `-p`. Every command
takes `--help`, problem names tab-complete in any case (`03_loop` finds
`03_Loop_11`), and the commands that hand back data take `--json`. `submit`
won't spend a submission on code that doesn't build and exits 0 only on full
marks, so `kafae submit 01_Str_11.cpp && git commit -am 'solve 01_Str_11'`
does the right thing; a [Digital](https://github.com/hneemann/Digital) `.dig`
circuit submits like any other file.

## Offline

```console
$ kafae sync                      # every statement, PDF, testcase and attachment, cached
$ export KAFAE_OFFLINE=1          # on the train
$ kafae test 01_Str_11.cpp        # the grader's testcases, run here
```

Sync while you have signal. `KAFAE_OFFLINE=1` (or `--offline`) then serves
`problems`, `view`, `new`, `run` and `test` from the cache and never opens a
socket; the rest say they need the grader rather than hanging on a timeout.
The cache is per grader, and `kafae clean` clears it.

## Your own API calls

`kafae token` prints the token of the cached session, and nothing else, so the
grader's API is one header away:

```bash
curl -H "Authorization: Bearer $(kafae token)" "$KAFAE_URL/api/v1/me"
```

The token is the one `kafae login` cached, so it dies with the same 12h clock;
`kafae token --json` adds the grader url and login it belongs to.

## Environment

`KAFAE_URL` and `KAFAE_USER` pin the grader and your login, so a course
directory can name both and a lost token costs a password rather than a setup.
`KAFAE_CXX` / `KAFAE_CC` pick the local compiler and `KAFAE_CXXFLAGS` /
`KAFAE_CFLAGS` replace its flags, which otherwise mirror the grader's plus
`-DLOCAL`, so `#ifdef LOCAL` debug output strips itself on submit.

## Platforms

Linux, macOS and Windows. Everything that talks to the grader works
everywhere; the local `run` / `test` / `submit` check wants a gcc-flavoured
compiler: any g++ on Linux, MinGW g++ on Windows (not MSVC), and on macOS
Apple clang or, for code using `bits/stdc++.h`, a real gcc (`brew install
gcc`, then `KAFAE_CXX=g++-15`).

The nix package installs shell completions; with a plain release binary they
come from the binary itself, one line in your shell's rc:

```bash
source <(COMPLETE=bash kafae)                  # ~/.bashrc
source <(COMPLETE=zsh kafae)                   # ~/.zshrc
source (COMPLETE=fish kafae | psub)            # ~/.config/fish/config.fish
```

```powershell
# $PROFILE
$env:COMPLETE = "powershell"; kafae | Out-String | Invoke-Expression; Remove-Item Env:\COMPLETE
```

## Support

This CLI is primarily built for and intended to be used with [Chula](https://www.chula.ac.th/en/)'s Computer Engineering courses. It may work with other independently hosted graders, but that isn't guaranteed. If you know a bit of Rust and wanna make it work for you too, see the Contributing section below!

## Contributing

Issues and PRs are welcome! Commit style lives in
[CONTRIBUTING.md](CONTRIBUTING.md).

---

<p align="center">Made with ❤️ by <a href="https://github.com/MiniHarinn">@MiniHarinn</a> (CEDT04) and a dangerous amount of kafae(ine)</p>
