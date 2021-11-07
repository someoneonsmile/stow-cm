# config manager (simple impl of gnu-stow)

## USEAGE

```sh
# -i install pack
# -d remove pack
# -r reload pack
# -f force replace target file
stow -i ./nvim /foo/bar
stow -i ./nvim /foo/bar -f
stow -d ./nvim /foo/bar
stow -r ./nvim /foo/bar
stow -r ./nvim /foo/bar -d /bar
stow -r ./*
```

## CONFIG

- config file location

    - `./.stowrc`
    - `{stow pack dir}/.stowrc`

- config file format

```toml
# toml format

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
