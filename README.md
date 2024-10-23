# config manager (gnu-stow like)

## USEAGE

```
config manager (gnu-stow like)

Usage: stow-cm <COMMAND>

Commands:
  install  Install packs
  remove   Remove packs
  reload   Reload packs (remove and install)
  clean    Scan and clean all symbol that link to pack from pack target
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
stow-cm clean ./nvim /path/to/pack
stow-cm encrypt ./nvim /path/to/pack
stow-cm decrypt ./nvim /path/to/pack

stow-cm install ./*
```

## CONFIG

### Location

- `$XDG_CONFIG_HOME/stow-cm/config.toml`: common config
- `{stow pack dir}/stow-cm.toml`: pack config

> note: it not use the pack/pack_sub_path/stow-cm.toml

### Format

```toml
# toml format

# The follow environment variable will be injected
# PACK_ID: hash of the package path.
# PACK_NAME: represents the last level of the package path.

# when targe is none both in pack_config and common_config, it will skip link the dir_tree
# env var support the default value: ${env:-default}
# target = '~'

# ! represents unset the value
# target = '!'

# default
target = '${XDG_CONFIG_HOME:-~/.config}/${PACK_NAME}/'

# override
override = [
    # single quotes not excaping
    '.*\.lua',
]

# ignore
ignore = [
    '.*\.md',
    ".*\\.lock",
]

# default, create a tree-folding symlink
fold = true

[init]
type = '[Bin/Python/Make/Lua/Shell/ShellStr]'
# Bin/Shell/Python/Make/Lua: file path relate on the pack
# ShellStr: string
content = 'pack_sub_path/to'

[clear]
type = '[Bin/Python/Make/Lua/Shell/ShellStr]'
# Bin/Shell/Python/Make: file path relate on the pack
# Script: string
# content = 'pack_sub_path/to'
content = '''
echo ${PACK_ID}
echo ${PACK_NAME}
if [ -d /path/to ]; then
  rm -rf /path/to
fi
'''

[crypted]
# default
enable = false
# default
decrypted_path = '${XDG_DATA_HOME:-~/.local/share}/stow-cm/${PACK_ID}/decrypted/'
# default
left_boundry = '&{'
# default
right_boundry = '}'
# support ChaCha20-Poly1305 | AES-128-GCM | AES-256-GCM
# default ChaCha20-Poly1305
crypted_alg = 'ChaCha20-Poly1305'
key_path = '/path/to/key'
```

## TODO

- [x] ignore
- [x] override
- [x] init/clear script
- [x] valid conflict before install
- [x] protect mode (don't excute in non stow dir)
- [x] if target is none just skip stow dir
- [x] remove refact
- [x] github action (auto archive)

- [x] crypted support
- [x] crypted support skip binary file
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
