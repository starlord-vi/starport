# Starport local development

- Use CLI tools only. The user runs and tests the game; do not launch it
  or use computer-use automation unless the user explicitly changes this.

- Keep the inherited GPL-3.0-or-later license, history and contributor credits.
- Keep original game files unchanged. Game data, captures, caches and binaries
  belong outside this repository. Run `cargo xtask policy` before delivery.
- Keep this repository local. Do not push or post to GitHub unless the user asks.
- Read and test imported changes. Retain their source URL and exact revision.
- Do not copy GPL-2.0-only XCL source into this GPL-3 project.
- Do not skip assertions, invent successful GPU results, drop rendering passes
  to improve FPS, or call a lit-looking fallback ray tracing.
- Separate locally reproduced results from upstream reports. Report ignored
  tests and internal GPU/data-dependent skips alongside real test counts.
- Runtime evidence goes under the adjacent `runtime/starport` directory; the
  local build currently uses the adjacent `compat/starport_build` directory.
