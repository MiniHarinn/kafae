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
The cache is per grader, and `kafae clean` clears it. A problem that ships a
file of its own (a digital-logic template, say) is cached like the PDF, and
`kafae view` prints the path it landed at. `kafae new <problem> -t attachment`
starts your solution from that file instead of from a template.

## Your own API calls

`kafae token` prints the cached session token, so the grader's API is one
header away:

```bash
curl -H "Authorization: Bearer $(kafae token)" "$KAFAE_URL/api/v1/me"
```

It dies on the same 12h clock as the login that cached it. `kafae token --json`
adds the grader url and login it belongs to.

## Configuration

`kafae login` writes a commented `config.toml` and fills in the grader you
just logged into. Nothing else touches the file, and it never holds a token.

```console
$ kafae config path      # where the config, sessions, cache and templates live
$ kafae config show      # every setting, its value, and where it came from
$ kafae config edit      # $VISUAL / $EDITOR, writing the file first if there is none
```

```toml
version = 1
current = "ce"                    # which grader every command talks to

cc = "gcc"                        # this machine's compilers and flags
cxx = "g++"
cflags = "-O2 -std=c99 -DCONTEST -DLOCAL -lm -Wall"
cxxflags = "-O2 -std=c++17 -DCONTEST -DLOCAL -lm -Wall"
template = "default"              # what `kafae new` starts from without -t

[grader.ce]                       # one table per grader, named however you like
url = "https://grader.cp.eng.chula.ac.th"
login = "6xxxxxxx21"

[grader.algo]                     # a table can also set template, cflags, cxxflags
url = "https://algo.example.ac.th"
login = "6xxxxxxx21"
cxxflags = "-O2 -std=c++20 -DCONTEST -DLOCAL -lm -Wall"
template = "attachment"
```

A grader is one server you have an account on, so a second course on a
second server is a second table:

```console
$ kafae graders                  # every grader, with what is left of each session
$ kafae use algo                 # switch current; with two graders, use alone swaps
$ kafae --grader algo problems   # one command, without moving current
$ kafae login --grader algo      # add a grader, or re-login to one
$ kafae logout                   # forget this grader's token, --all for every grader
```

A flag beats `KAFAE_<KEY>` beats the grader's own table beats the top-level
key beats kafae's default, and `kafae config show` prints which one won.

Your login survives the move to per-grader sessions. Going back to an older
kafae costs one more `kafae login`.

## Environment

`KAFAE_URL` and `KAFAE_USER` override the selected grader's url and login.
Unlike the file they pick a different account outright, so unexporting one
brings the old session back alive. `KAFAE_GRADER` picks the grader itself,
for one shell or one command. `KAFAE_TEMPLATE`, `KAFAE_CXX`, `KAFAE_CC`,
`KAFAE_CXXFLAGS` and `KAFAE_CFLAGS` override their matching key, same as
before. `KAFAE_OFFLINE` is `--offline` in an export. None of them is
deprecated, and none gets written back to the file.

`KAFAE_HOME=<dir>` moves config, templates, sessions and cache there
together. It's the escape hatch on a shared machine; your tokens live under
`sessions/`, so don't point it at a directory you commit.

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
