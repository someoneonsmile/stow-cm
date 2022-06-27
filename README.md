# config manager (simple impl of gnu-stow)

## USEAGE

```sh
# -i install pack
# -d remove pack
# -r reload pack
stow -i ./nvim /foo/bar
stow -i ./nvim /foo/bar
stow -d ./nvim /foo/bar
stow -r ./nvim /foo/bar
stow -r ./nvim /foo/bar -d /bar
stow -r ./*
```

## CONFIG

### Location

- `$XDG_CONFIG_HOME/stow/config`: common config
- `{stow pack dir}/.stowrc`: pack config

> note: it not use the pack/pack_sub_path/.stowrc

### Format

```toml
# toml format

# target = "$XDG_CONFIG_HOME/stow/"
target = "~"

# override
over = [
    ".*\\.lua",
]

# ignore
ignore = [
    ".*\\.md",
    ".*\\.lock",
]

# default, create a tree-folding symlink
fold = true,

[init]
type = "[Bin/Python/Make/Lua/Shell/ShellStr]"
# Bin/Shell/Python/Make/Lua: file path on the pack
# ShellStr: string
content = "file path on the pack"

[clear]
type = "[Bin/Python/Make/Lua/Shell/ShellStr]"
# Bin/Shell/Python/Make: file path on the pack
# Script: string
content = "file path on the pack"
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

- [x] start script
- [ ] tracing log

- [ ] valid conflict before install
- [ ] remove refact

- [ ] attr macro Merge
- [ ] override
