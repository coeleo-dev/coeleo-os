# pkg

`.coe` format (phase 14): `COE1` + name + version + ELF + Ed25519. The signed region is everything except the final 64 signature bytes.

**Folder:** `userspace/libs/pkg`  
**Userspace workspace:** yes  
**Consumers:** `userspace/apps/sh` (`pkg install` / `remove`); on the host, [`coe-pack`](../tools-coe-pack.md) depends on this crate **without** `build-std`.

`no_std`. Dependencies: `ed25519-dalek` (`zeroize`, no default features). The public key is baked in:

```text
PUBKEY = include_bytes!("../../../../tools/coe-pack/dev.pub")
```

## Constants

| Name | Value |
| --- | --- |
| `MAGIC` | `b"COE1"` |
| `NAME_MAX` | 8 |
| `HDR` | 21 |
| `SIG_LEN` | 64 |
| `PAYLOAD_MAX` | 256 KiB |

Name: 1..=8 characters `[a-z0-9]`.

## Types and functions

```text
struct Pkg<'a> {
    name, ver_major, ver_minor,
    payload, signed, signature
}
enum PkgErr { Truncated, Magic, Name, Size }

pubkey() -> &'static [u8; 32]
parse(blob: &[u8]) -> Result<Pkg, PkgErr>
verify(pkg, pubkey) -> bool
```

`parse` requires `blob.len() == HDR + payload_len + SIG_LEN`. `verify` uses `VerifyingKey::verify_strict`.

`sh` reads the file in 4096-byte chunks into a static `PAYLOAD_MAX` buffer, then `parse` + `verify` + writes to `/bin/<name>`.

## Do not

- Do not treat `dev.key` / `dev.pub` as a production secret (demo seed).
- Do not compile this crate via `cargo --manifest-path userspace/Cargo.toml` **together** with the host packer in a way that pulls `std` and `build-std` at once — the packer is built in `tools/coe-pack`.
- Do not raise `PAYLOAD_MAX` without the spec and the `sh` buffer.
