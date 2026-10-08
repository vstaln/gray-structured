<p align="center">
  <img src="assets/gray-logo.svg" alt="gray" width="96">
</p>
<h1 align="center">gray-structured</h1>
<p align="center">A `structured` tool that returns parsed JSON matching your schema, via a constrained sub-turn.</p>
<p align="center">
  <a href="https://github.com/vstaln/gray-structured/blob/main/LICENSE"><img alt="MIT License" src="https://img.shields.io/badge/license-MIT-blue.svg"></a>
  <img alt="gray plugin" src="https://img.shields.io/badge/gray-plugin-7aa2f7.svg">
  <img alt="rust" src="https://img.shields.io/badge/built%20with-rust-orange.svg">
</p>

A `structured` tool: runs a host sub-turn constrained to a JSON schema and returns the parsed JSON.

## What it does

`structured {prompt, schema}` sends `host/run` (capability `host.turn`)
the prompt `"Respond with ONLY a JSON object matching this JSON schema.
Schema: <schema>\nTask: <prompt>"`, extracts the first balanced `{…}` block
from the reply (braces inside strings don't count), and returns it
pretty-printed as the tool result. No parseable JSON → `is_error` carrying
the raw reply. 25s cap (under the host's own 28s host/run deadline).

The output shape is a per-call argument — each invocation runs a nested
model turn constrained to the supplied schema.

## Wire methods

Host→sidecar: `plugin/manifest`, `tool/call`, `plugin/shutdown`.
Sidecar→host: `host/run` — requires capability `host.turn`.

## Install + consent

```sh
gray plugin install structured
gray plugin capabilities structured --all   # grants host.turn
```

Ungranted, the tool still answers — with an is_error explaining the host
turn never came back — rather than hanging.

## Develop

```sh
cargo test
cargo build --release
gray account check      # entry point + manifest handshake
```

---
Part of the [gray](https://github.com/vstaln/gray) plugin ecosystem —
the open-source AI agent harness. <https://gray.alignment.id>
