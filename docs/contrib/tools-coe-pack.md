# coe-pack

**Host** tool that produces `.coe`. Not a userspace workspace member. Not in the ISO.

**Folder:** `tools/coe-pack`  
**Dependency:** `pkg = { path = "../../userspace/libs/pkg" }` (with `std`, without `build-std`).

Build: `make -C tools/coe-pack` (release). `make userspace` already does this and runs:

```text
coe-pack pack hello apps/hello/hello -o libs/pkg/hello.coe
coe-pack bad  hello apps/hello/hello -o libs/pkg/bad.coe
```

`.coe` files go on the disk under `::pacotes/`.

## Commands

```text
coe-pack pack <name> <elf> -o <out.coe>   # sign
coe-pack bad  <name> <elf> -o <out.coe>   # same, last signature byte inverted
coe-pack write-pub                        # write dev.pub (and dev.key if missing)
```

`<name>`: 1..=8, `[a-z0-9]`. ELF ≤ `pkg::PAYLOAD_MAX`.

`bad` serves the phase 14 test (`pkg: bad signature`).

## Keys

`dev.key` is a **demo** seed, not a production secret. The constant in the source (`SEED`) must match `tools/coe-pack/dev.key` if the file exists. `dev.pub` (32 bytes) is what [pkg](libs/pkg.md) embeds in the ELF `sh`.

To rotate the key: `write-pub`, update `dev.pub` in git if it is the shared development key, rebuild `sh`.

## Do not

- Do not `cargo build --manifest-path userspace/Cargo.toml -p coe-pack`.
- Do not treat `dev.key` as a production secret.
- Do not change the `.coe` layout without the spec and `pkg::parse`.
