# config manager (simple impl of gnu-stow)

## USEAGE

- config file location

    - `./.stowrc`
    - `{stow pack dir}/.stowrc`

- config file format

```toml
# toml format

target = "~"
ignore = [
    ".*\\.md",
    ".*\\.lock",
]
```

## TODO

- [ ] 模块
- [ ] 错误处理
- [ ] 日志
- [ ] 配置文件
- [ ] ignore 正则
- [ ] 控制台颜色
- [ ] unused 处理
