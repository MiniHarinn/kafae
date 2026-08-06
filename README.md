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
$ kafae new 01_Str_11             # writes 01_Str_11.cpp
$ kafae run 01_Str_11.cpp         # compile and run here, not on the grader
$ kafae test 01_Str_11.cpp        # run the grader's testcases locally
$ kafae submit 01_Str_11.cpp      # compile check, submit, wait for the verdict
$ kafae status 1234               # check a verdict later
```

`new` names the file after the problem, so everything downstream infers the
problem from the filename. It starts from a template: builtins ship in
[templates/](templates) (`kafae new -t py` picks one by name), and any
`name.ext` file you drop in your config's `kafae/templates` folder is offered
too, shadowing a builtin with the same name. `{name}` and `{title}` are filled
in; the file's extension comes from the template. `kafae templates` lists them
and prints the folder. `submit` refuses to spend a submission on code that
does not build, and exits 0 only on full marks, so this works:

```console
$ kafae submit 01_Str_11.cpp && git commit -am 'solve 01_Str_11'
```

`test` runs against the grader's own testcases, fetched once and then
cached; it only works on problems where the grader shares them.

Local compile flags mirror the grader's, plus `-DLOCAL` so `#ifdef LOCAL`
debug output strips itself on submit (override with `KAFAE_CXXFLAGS` /
`KAFAE_CFLAGS`; pick the compiler binary with `KAFAE_CXX` / `KAFAE_CC`,
say `KAFAE_CXX=g++-14` for Homebrew gcc). Completions for bash, zsh and
fish come with the package, and problem names tab-complete from the last
list the grader sent.

## Platforms

Linux, macOS and Windows are all supported; everything that talks to the
grader works everywhere. The local `run` / `test` / `submit` check want a
gcc-flavoured compiler: any g++ on Linux, MinGW g++ on Windows (MSVC is
not supported; the grader itself builds with g++), and on macOS either
Apple clang or, for code using `bits/stdc++.h`, a real gcc
(`brew install gcc`, then `KAFAE_CXX=g++-15`).

The nix package installs completions; with a plain release binary, they
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
