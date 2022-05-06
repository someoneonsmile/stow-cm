# config manager (simple impl of gnu-stow)
## USEAGE

```sh
# -i install pack
# -d remove pack
# -r reload pack
# -f force replace target file, if the pack config not config the force option
stow -i ./nvim /foo/bar
stow -i ./nvim /foo/bar -f
stow -d ./nvim /foo/bar
stow -r ./nvim /foo/bar
stow -r ./nvim /foo/bar -d /bar
stow -r ./*
```

## CONFIG

- config file location and find order

    - `$XDG_CONFIG_HOME/stow/config`
    - `./.stowrc`
    - `{stow pack dir}/.stowrc`

    > note: it not use the pack/pack_sub_path/.stowrc

- config file format

```toml
# toml format

# target = "$XDG_CONFIG_HOME/stow/"
target = "~"
force = true
ignore = [
    ".*\\.md",
    ".*\\.lock",
]
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
