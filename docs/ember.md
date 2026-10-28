# Ember: Horizon's mod loader

Ember is the name of Horizon's mod loader — the part that finds a mod, checks it, loads
it into a sandbox and calls its hooks at the points the game allows. It is the loader
behind the `.hmod` format described in [mods.md](mods.md), and the same loader runs on
every platform Horizon builds for.

There is no separate "Android mod" and "PC mod". An `.hmod` is one WebAssembly component
built once against a versioned WIT contract, and Ember loads it identically on a phone
and on a desktop. **A mod that supports Horizon on Android supports Horizon on PC, and
the reverse**, because there is nothing platform-specific in a mod to differ.

## What Ember is not

Ember is not a Mixin loader. It does not patch Minecraft's classes — Horizon has none,
it is Rust — and it does not rewrite the game's code. A mod adds behaviour through a
fixed list of hooks, which is why the parity harnesses can keep testing the same game
whether mods are on or off.

Fabric and NeoForge jars do not run. Their code calls into Minecraft's Java classes.
What does carry over from a Java mod is its data and assets (see
[modpacks.md](modpacks.md)).

## Why it runs on a phone

A phone's SELinux policy refuses to make memory executable, so a just-in-time compiler
cannot run there. Ember handles this by compiling to **Pulley bytecode** instead of
machine code: the same Cranelift compiler, pointed at the interpreter target. Nothing is
ever mapped executable, and the interpreter that runs the bytecode is part of the
runtime. A desktop build points the same compiler at the host and gets native code.

The sandbox is unchanged either way: the same fuel, the same 64 MiB memory cap, the same
capability list. A mod cannot tell which one it is running under.

## The parts

| Part | What it is |
|---|---|
| `.hmod` | A WebAssembly component with a `horizon-mod.toml` manifest inside. One file, every platform. |
| `horizon-mod-sdk` | The Rust SDK a mod is written against: the WIT world, the host imports, the hooks. |
| `cargo horizon-mod` | The authoring tool: `new`, `build`, `run`, `check`. |
| Ember (this document) | The loader: discovery, verification, sandbox, hook dispatch. |
| The registry | `ludovic111/horizon-mods`: reviewed entries, one `mods/<id>.toml` each, releases per version. |

## How a mod is loaded

1. **Discovery.** `import/mods/*.hmod` on a phone, the game home's `mods/` on a desktop.
   The launcher's Mods page installs into the same folder.
2. **Manifest.** `horizon-mod.toml` is read first, without running any code. It names
   the id, the API version, the side (`client` or `server`) and the capabilities the mod
   wants.
3. **Refusal.** A mod built for a different API version, for the other side, or with a
   capability this build does not grant is not loaded, and the reason is logged. A mod
   is never half-loaded.
4. **Verification.** When it came from the registry, its SHA-256 and, if the author
   signed it, its minisign signature are checked before the file is read. A revoked file
   is refused even if it is asked for by name.
5. **Sandbox.** A store is made with the fuel and the memory cap. The host imports the
   mod is allowed are linked; everything else is a load error the mod's author sees when
   they build.
6. **Start.** The mod's `init` runs once. A content-only mod has no code to run and is
   read before the world loads.

## The hooks

| Hook | When |
|---|---|
| `init` | Once, when the mod starts. |
| `tick` | Twenty times a second, with the game's tick count. |
| `frame` | Once per drawn frame, with the frame's delta. |
| `chat` | A chat message was sent or received. |
| `command` | A `/command` was typed; a mod may handle its own. |

What a hook may do is the capability list, not the hook: `game-read` to see the player
and the world, `hud` to draw, `chat` to write, `storage` to keep a file of its own. There
is no file, network or clock access outside those.

## Writing one

```sh
cargo install --path crates/cargo-horizon-mod     # once
cargo horizon-mod new my-mod --name "My Mod"
cd my-mod
cargo horizon-mod run                              # in the game, reloaded on rebuild
cargo horizon-mod build                            # my-mod.hmod
cargo horizon-mod check                            # the registry's own checks
```

`cargo horizon-mod new` writes a working mod: a manifest, a crate that depends on the
SDK, and a `src/lib.rs` with one hook filled in. The SDK's own example,
`crates/horizon-mod-sdk/examples/coords-hud`, is a complete mod that reads the player's
position, draws a panel and answers two commands.

### The manifest

```toml
id = "my_mod"
name = "My Mod"
version = "0.1.0"
api = "0.1"
side = "client"
capabilities = ["game-read", "hud"]
```

### A hook

```rust
impl Guest for MyMod {
    fn tick(clock: Clock, player: Player) {
        if clock.ticks % 20 == 0 {
            log(&format!("{} at {} {} {}", player.name, player.x, player.y, player.z));
        }
    }
}
```

## Publishing

The registry takes a pull request against `ludovic111/horizon-mods`, or a submission
through [playhorizon.dev/mods/submit](https://playhorizon.dev/mods/submit). Its CI runs
the file's own checks, a trial run in the sandbox and the index it would publish; a
reviewer reads the entry, the source and the capability list. Merging publishes it, and
the launcher's Mods page sees it on the next refresh.

Every release asset is named `<id>-<version>.hmod` and tagged `<id>-<version>`, which is
what the launcher builds its download address from. The entry carries the SHA-256 of the
file it points at, and the launcher refuses a file that does not match.

## Where the pieces live

- `crates/horizon-mods` — Ember: the manifest, the loader, the sandbox, the registry index.
- `crates/horizon-mod-sdk` — the WIT contract and the Rust bindings a mod is written against.
- `crates/cargo-horizon-mod` — `cargo horizon-mod`, and the template `new` writes.
- `crates/horizon-mod-sdk/examples/coords-hud` — a complete mod.
- `crates/horizon-client/src/game/mods.rs` — where the game calls Ember's hooks.
- `crates/horizon-client/src/mod_registry.rs` — the launcher's side: browse, download, verify, install.
