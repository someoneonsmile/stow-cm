# changelog

## 0.13.0

- refactor: clippy (#17)
- fix: when ignore or override regex not valid case throw the error (#18)
- fix: encrypt command deal for ignore_re not right (#19)
- chore: remove lazy_static crate (#20)
- chore!: spell check (#21)

### BREAKING CHANGE!

- rename config file `crypted` section to `encrypted`
- rename config file `crypted.crypted_alg` section to `encrypted.encrypted_alg`
