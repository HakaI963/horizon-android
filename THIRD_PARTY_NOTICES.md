# Third-party notices

## wgpu-hal

`third_party/wgpu-hal` is [wgpu-hal](https://github.com/gfx-rs/wgpu) 30.0.1 by the gfx-rs
developers, with one upstream fix applied
([gfx-rs/wgpu#10124](https://github.com/gfx-rs/wgpu/pull/10124)). It is licensed under MIT or
Apache-2.0; both licenses are in that folder (`LICENSE.MIT`, `LICENSE.APACHE`).

## Pixelify Sans

`crates/horizon-theme/fonts/PixelifySans-*.ttf` is [Pixelify Sans](https://github.com/eifetx/Pixelify-Sans)
by The Pixelify Sans Project Authors, licensed under the SIL Open Font License 1.1
(`crates/horizon-theme/fonts/OFL-PixelifySans.txt`).

## Pixelarticons

`crates/horizon-theme/icons/pixel/*.svg` are icons from
[Pixelarticons](https://github.com/halfmage/pixelarticons) by Gerrit Halfmann, under the MIT
license (`crates/horizon-theme/icons/pixel/LICENSE-pixelarticons.txt`).

## imgui-wgpu

`crates/horizon-imgui` is the wgpu backend of
[imgui-wgpu](https://github.com/Yatekii/imgui-wgpu-rs) 0.28.0 by Steven Wittens, vendored and
updated for wgpu 30 (the published crate stops at 29). It is MIT-licensed, and
`crates/horizon-imgui/LICENSE-upstream.md` is the notice as upstream ships it.

## Dear ImGui

The on-screen controls are drawn by [Dear ImGui](https://github.com/ocornut/imgui) through the
[`imgui`](https://github.com/imgui-rs/imgui-rs) Rust binding. Nothing of it is vendored here — the
binding's `imgui-sys` crate carries the C++ source and its own MIT and Apache-2.0 licenses, and
compiles it for the phone with the NDK's clang++.
