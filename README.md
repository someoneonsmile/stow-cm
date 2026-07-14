# Config manager (gnu-stow like)

## USAGE

```
config manager (gnu-stow like)

Usage: stow-cm <COMMAND>

Commands:
  install  Install packs
  remove   Remove packs
  reload   Reload packs (remove and install)
  clean    Scan and clean all symlinks that link from pack to pack target
  encrypt  Scan files in the given pack for replacement variables, encrypt them, and replace them back to the original files
  decrypt  Scan files in the given pack for replacement variables, decrypt them, and replace them back to the original files
  help     Print this message or the help of the given subcommand(s)

Options:
  -h, --help     Print help
  -V, --version  Print version
```

```sh
stow-cm install ./nvim /path/to/pack
stow-cm remove ./nvim /path/to/pack
stow-cm reload ./nvim /path/to/pack
stow-cm clean ./nvim /path/to/pack   # only works for symlink-mode packs
stow-cm encrypt ./nvim /path/to/pack
stow-cm decrypt ./nvim /path/to/pack

stow-cm install ./*
```

## INSTALL

### Arch

```sh
paru -S stow-cm-bin
paru -S stow-cm-nightly-bin
# or with yay
yay -S stow-cm-bin
yay -S stow-cm-nightly-bin
```

## CONFIG

### Location

- `$XDG_CONFIG_HOME/stow-cm/config.toml`: common config
- `{stow pack dir}/stow-cm.toml`: pack config

> note: it not uses the pack/pack_sub_path/stow-cm.toml

### Format

```toml
# toml format

# The following environment variables will be injected
# PACK_ID: hash of the package path.
# PACK_NAME: represents the last level of the package path.

# when target is none both in pack_config and common_config, it will skip link the dir_tree
# env var support the default value: ${env:-default}
# target = '~'

# '!' unset the value (override global/default to None)
# works for all string/path fields: target, encrypted.decrypted_path, encrypted.key_path, etc.
# target = '!'

# default
target = '${XDG_CONFIG_HOME:-~/.config}/${PACK_NAME}/'

# override
override = [
    # single quotes not escaping
    '.*\.lua',
]

# ignore
ignore = [
    '.*\.md',
    ".*\\.lock",
]

# '!' in array: truncate at '!', only keep elements before it
# default merge strategy for arrays is append (pack + global)
# place '!' at the end to override (discard global values)
# ignore = ['.*\.md', '!']  # override: only '.*\.md', discard global ignore
# ignore = ['!']            # clear all ignore patterns

# default, create a tree-folding symlink
fold = true

# default, use symlink, another mode is 'copy'
# NOTE: copy mode is incompatible with the `clean` command (clean only scans symlinks).
#       Use `remove` to uninstall copy-mode packs.
mode = 'symlink'

[init]
type = '[Bin/Python/Make/Lua/Shell/ShellStr]'
# Bin/Shell/Python/Make/Lua: file path relate on the pack
# ShellStr: string
content = 'pack_sub_path/to'

[clear]
type = '[Bin/Python/Make/Lua/Shell/ShellStr]'
# Bin/Shell/Python/Make/Lua: file path relate on the pack
# ShellStr: string
# content = 'pack_sub_path/to'
content = '''
echo ${PACK_ID}
echo ${PACK_NAME}
if [ -d /path/to ]; then
  rm -rf /path/to
fi
'''

[encrypted]
# default
enable = false
# default
decrypted_path = '${XDG_STATE_HOME:-~/.local/state}/stow-cm/${PACK_ID}/decrypted/'
# default
left_boundary = '&{'
# default
right_boundary = '}'
# support ChaCha20-Poly1305 | AES-128-GCM | AES-256-GCM
# default ChaCha20-Poly1305
encrypted_alg = 'ChaCha20-Poly1305'
key_path = '/path/to/key'
```

## TODO

- [x] ignore
- [x] override
- [x] init/clear script
- [x] valid conflict before install
- [x] protect mode (don't execute in non stow dir)
- [x] if target is none just skip stow dir
- [x] remove refact
- [x] github action (auto archive)

- [x] encrypted support
- [x] encrypted support skip binary file
- [ ] attr macro Merge

- [ ] more test and testable

- [x] doc update
- [x] refactor: clear cli command
- [x] pack unset global or default target value

- [ ] log level from cli args

- [ ] split override, bak file

- [x] unpack pack from args

- [x] move .stow-cm-track to $XDG_STATE_HOME

- [ ] pack related properties combine into struct

- [x] support file copy mode

- [x] encrypted decrypted ignore binary file

- [x] sh auto complete
