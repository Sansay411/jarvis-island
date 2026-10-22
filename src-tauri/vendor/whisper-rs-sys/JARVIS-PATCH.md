# whisper-rs-sys 0.15.0 — local patch

Copy of the crates.io release, wired in through `[patch.crates-io]` in
`src-tauri/Cargo.toml`. Changes:

1. `src/bindings.rs`: removed the three `const _: () = { … }` layout-assertion
   blocks for glibc types (`_G_fpos_t`, `_G_fpos64_t`, `_IO_FILE`; 69 lines,
   done with a perl one-liner over the blocks). The bundled bindings were
   generated on Linux, and those assertions fail on Windows/MSVC; whisper.cpp
   never uses the types. All layout assertions for whisper's own structs are
   kept and pass on Windows x64. This lets the crate build with
   `WHISPER_DONT_GENERATE_BINDINGS=1`, so no libclang is required.
2. Removed ggml backends JARVIS does not build (CANN, CUDA, Metal, MUSA,
   SYCL, WebGPU, zDNN, OpenCL, Hexagon, ZenDNN). They are only added by CMake
   when their `GGML_*` option is on. CPU, BLAS, RPC and Vulkan remain.
3. `src/bindings.rs`: the C enum type aliases of ggml/whisper (22 of them,
   `pub type ggml_… / whisper_… = c_uint`) changed to `c_int`. MSVC gives C
   enums the type `int`, and whisper-rs declares its Rust enums `repr(i32)`
   on Windows MSVC; the Linux-generated `c_uint` made it fail to compile.
   Size and layout are unchanged (4 bytes).
4. Lint-only: `#![allow(unnecessary_transmutes)]` in `src/lib.rs` and
   `#[allow(unused_mut)]` in `build.rs`, so a path dependency does not flood
   the build with warnings.

To update: copy the new release from the cargo registry and repeat these
steps. Optimisation flags for MSVC live in `src-tauri/.cargo/config.toml`.
