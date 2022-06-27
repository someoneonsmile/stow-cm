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

# env var support the default value: ${env:-default}
# target = '${XDG_CONFIG_HOME:-~/.config}/stow-cm/'
target = '~'

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
fold = true,

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
if [ -d /path/to ]; then
  rm -rf /path/to
fi
'''
```

## TODO

- [ ] 模块
- [ ] 错误处理
- [ ] 异步协程
- [ ] 日志
- [ ] 配置文件
- [ ] ignore 正则
- [ ] 控制台颜色
- [ ] unused 处理
- [ ] git ignore
- [ ] backup replace file
- [ ] 加密
- [ ] protect mode (don't excute in non stow dir)
- [ ] github action (auto archive)

- [x] init/clear script
- [ ] tracing log

- [ ] valid conflict before install
- [ ] remove refact

- [ ] attr macro Merge
- [ ] override

- [ ] rename project and config file
- [ ] command stderr output
