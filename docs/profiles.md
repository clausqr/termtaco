# Profiles

A good dial for a given source takes a handful of flags together; `--profile
NAME` loads a named bundle of them so you don't have to retype the combination
every time:

```sh
ping 8.8.8.8 | termtaco --profile ping
```

`ping` ships built in (it's the long-form command from the README's
examples, saved as a profile). Flags given on the command line alongside
`--profile` override its values, the same way `--theme` overrides the theme
file:

```sh
ping 8.8.8.8 | termtaco --profile ping --kalman-r 900
```

For a custom profile, write `~/.config/termtaco/profiles/NAME`: one
`flag = value` or bare `flag` line per option (bare for flags that take no
value, like `kalman` or `zero`), e.g.

```
parser = ping
title = ping ms
zero
kalman
kalman-r = 1300
```

`--print-profile NAME` dumps a built-in as a starting point, same idea as
`--print-theme`:

```sh
mkdir -p ~/.config/termtaco/profiles
termtaco --print-profile ping > ~/.config/termtaco/profiles/myping
```

A `NAME` containing a `/` is read as a literal path instead, e.g. `--profile
./myping.profile`, without needing to install it anywhere first.
