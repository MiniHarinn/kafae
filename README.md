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

`kafae token` prints the token of the cached session, and nothing else, so the
grader's API is one header away:

```bash
curl -H "Authorization: Bearer $(kafae token)" "$KAFAE_URL/api/v1/me"
```

The token is the one `kafae login` cached, so it dies with the same 12h clock;
`kafae token --json` adds the grader url and login it belongs to.

## Configuration

`kafae login` writes a commented `config.toml` and fills in the grader it just
logged into. Nothing else ever writes it behind your back, and no token is kept
in it.

```console
$ kafae config path               # where the config, sessions, cache and templates live
$ kafae config show               # every setting, its value, and the layer it came from
$ kafae config edit               # $VISUAL / $EDITOR, writing the file if there is none
```

The file is `~/.config/kafae/config.toml` on Linux, `~/Library/Application
Support/kafae/config.toml` on macOS and `%APPDATA%\kafae\config.toml` on
Windows; `kafae config path` is the one that tells you, sessions and cache
included.

```toml
version = 1
current = "ce"                    # which grader every command talks to

cc = "gcc"                        # this machine's compilers and their flags
cxx = "g++"
cflags = "-O2 -std=c99 -DCONTEST -DLOCAL -lm -Wall"
cxxflags = "-O2 -std=c++17 -DCONTEST -DLOCAL -lm -Wall"
template = "default"              # what `kafae new` starts from when -t is absent

[grader.ce]                       # one table per grader; the name is yours
url = "https://grader.cp.eng.chula.ac.th"
login = "6xxxxxxx21"

# a table may also set template, cflags and cxxflags, for that grader alone
[grader.algo]
url = "https://algo.example.ac.th"
login = "6xxxxxxx21"
cxxflags = "-O2 -std=c++20 -DCONTEST -DLOCAL -lm -Wall"
template = "attachment"
```

A grader is one server you have an account on, so a second course on a second
server is a second table:

```console
$ kafae graders                   # every grader, with what is left of each session
$ kafae use algo                  # change `current`; with two configured, `use` alone swaps
$ kafae --grader algo problems    # for one command, without moving `current`
$ kafae login --grader algo       # add a grader, or re-login to one
$ kafae logout                    # forget this grader's token (`--all` for every grader)
```

Every setting resolves the same way: a flag, then `KAFAE_<KEY>`, then the
selected `[grader.<name>]`, then the top-level key, then kafae's own default.
`kafae config show` prints which one won, which is the answer to both "why is it
compiling with that" and "why did that submit to the wrong course".

Your login survives the upgrade to per-grader sessions, but going back to an
older kafae costs one `kafae login`: it cannot read the new session files.

### Environment

The variables are the ad-hoc layer and they outrank the file. None of them is
deprecated.

`KAFAE_URL` and `KAFAE_USER` override the selected grader's `url` and `login`,
so a course directory can name both. They select an account rather than
replacing one: the session for the account they name is the one used, and
unexporting them brings the other back, alive. `KAFAE_GRADER=<name>` selects a
configured grader for one shell or one command, which is what a per-directory
`.envrc` wants instead of a `current` every terminal shares. `KAFAE_TEMPLATE`
picks the template `kafae new` starts from when `-t` is absent. `KAFAE_CXX` /
`KAFAE_CC` pick the local compiler and `KAFAE_CXXFLAGS` / `KAFAE_CFLAGS` replace
its flags, which otherwise mirror the grader's plus `-DLOCAL`, so `#ifdef LOCAL`
debug output strips itself on submit. `KAFAE_OFFLINE` is the exported form of
`--offline`.

`KAFAE_HOME=<dir>` moves all four of config, templates, sessions and cache
together: `<dir>/config.toml`, `<dir>/templates/`, `<dir>/sessions/` and
`<dir>/cache/` — so your usual `~/.config/kafae/templates` is not on the list
while it is set, and `kafae config path` prints where each one went. It keeps a
course self-contained and it is the escape hatch on a shared machine — but
`<dir>/sessions/` is where your tokens live, so never point it at a directory
you commit.

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
