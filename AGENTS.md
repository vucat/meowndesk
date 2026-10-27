# AGENTS.md — Code Quality & Verification Guardrails (Meowndesk)

## 1. Rust Backend (`src-tauri/`)
- **Zero Clippy Warnings**: Run `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --all-features -- -D warnings` after every Rust task.
- **No Panics in Production**: Never use `.unwrap()` or `.expect()` in `src-tauri/src/` (allowed only in `src-tauri/tests/`). Use `?`, `Option`/`Result` combinators, or graceful fallback to `Reading::unavailable`.
- **Zero-Allocation Hot Loop**: The 1Hz collector loop (`collector.rs`, `system.rs`, `nvml.rs`) must reuse pre-allocated buffers and persistent `sysinfo::System` instances (refreshing only targeted components, never `refresh_all()`). Avoid unnecessary `.clone()` on large structs.
- **Non-Blocking IPC**: Never run synchronous OS/hardware queries (`sysinfo`, `nvml`, PDH, `/proc`) on the Tauri main/UI thread. Keep all hardware polling inside the dedicated background collector loop.
- **Cross-Platform Safety (`#[cfg(target_os)]`)**: Keep OS-specific code (`windows/` and `linux/`) strictly behind shared provider traits in `providers/mod.rs` so both platforms compile against the exact same internal types and signatures without dead-code or unused-import warnings.
- **Linux Build Tooling**: Install `mold` and `clang` (`sudo apt install -y mold clang`) for faster local Linux incremental builds and integration test linking. The repository-root `.cargo/config.toml` scopes `clang` + `-fuse-ld=mold` to `[target.x86_64-unknown-linux-gnu]` (Cargo reads config from the working directory upward, so this covers root `--manifest-path` commands and `src-tauri` runs); never move those flags to global `[build]` settings, or Windows cross-compilation (`cargo-xwin`) breaks. CI overrides the Linux linker with `cc` and clears `CARGO_ENCODED_RUSTFLAGS`, so no mold installation is required there.

## 2. Wire Contract & Data Integrity (`src-tauri/` <-> `src/`)
- **Wire Serialization Rules**: All wire field names are `camelCase`; enum strings are `lowercase_snake_case`.
- **Byte Strings & Precision**: All byte amounts and counters must serialize as unsigned decimal strings on the wire (`u64` in Rust <-> `string` parsed via `BigInt` in TypeScript). Never use floating-point JS `number` for raw byte totals, and never round percentages before evaluating status badge thresholds (`<80`, `80–<90`, `>=90`) or sorting.
- **Strict Type Safety**: Zero `any`, `@ts-ignore`, or `@ts-expect-error` in TypeScript. All incoming IPC payloads must pass `src/telemetry/validate.ts` boundary checks.

## 3. Frontend (`src/`)
- **60fps Canvas & React Rendering**:
  - `DotMatrixChart.tsx` must draw via `requestAnimationFrame`, scale for `window.devicePixelRatio`, and clean up `ResizeObserver` on unmount.
  - Keep high-frequency canvas drawing state in `useRef` rather than triggering extra React `useState` re-renders.
- **Deterministic Cleanup**: Every Tauri `Channel` subscription and window visibility listener registered in `client.ts` must have a deterministic cleanup/unsubscribe path.

## 4. Documentation & Context7 MCP Usage
- **Always Use Context7 First**: Before implementing or troubleshooting code that uses external libraries (`tauri` v2, `@tauri-apps/api` v2, `sysinfo`, `nvml-wrapper`, `battery`, `tokio`, or `react` 19), invoke the **Context7 MCP** (`resolve-library-id` -> `get-library-docs`) to verify current API signatures.
- **No Guesswork on Crate/Package APIs**: Do not guess or fall back to deprecated Tauri v1 (`@tauri-apps/api/tauri`) or older `sysinfo` (`SystemExt` / `ProcessExt`) traits. Always verify the installed version's exact API in `Cargo.toml` / `package.json` with Context7 first.

## 5. Definition of Done (Per Task Batch)
Before completing each task batch, all four checks must pass with 0 errors:
1. `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --all-features -- -D warnings`
2. `cargo test --manifest-path src-tauri/Cargo.toml`
3. `npm run typecheck && npm run lint`
4. `npx vitest run`
