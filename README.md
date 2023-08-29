# config manager (gnu-stow like)

## USEAGE

```sh
# -i install pack
# -d remove pack
# -r reload pack
stow-cm -i ./nvim /foo/bar
stow-cm -i ./nvim /foo/bar
stow-cm -d ./nvim /foo/bar
stow-cm -r ./nvim /foo/bar
stow-cm -r ./nvim /foo/bar -d /bar
stow-cm -r ./*
```

## CONFIG

### Location

- `$XDG_CONFIG_HOME/stow-cm/config.toml`: common config
- `{stow pack dir}/stow-cm.toml`: pack config

> note: it not use the pack/pack_sub_path/stow-cm.toml

### Format

```toml
# toml format

# The ${PACK_NAME} environment variable will be injected
# where PACK_NAME represents the last level of the package path.

# when targe is none both in pack_config and common_config, it will skip link the dir_tree
# env var support the default value: ${env:-default}
# target = '~'

# ! represents unset the value
# target = '!'

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
echo ${PACK_NAME}
if [ -d /path/to ]; then
  rm -rf /path/to
fi
'''
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

- [ ] encrypt
- [ ] attr macro Merge

- [ ] more test and testable

- [ ] doc update
- [ ] split override, bak file

- [x] pack unset global or default target value
