# gray-structured

`structured` tool: runs a host sub-turn constrained to a JSON schema and returns the parsed JSON. Port of pi's `structured-output` extension.

A sidecar plugin for [gray](https://github.com/vstaln/gray), scaffolded by
[gray-account](https://github.com/vstaln/gray-account).

## What it does

`structured {prompt, schema}` sends `host/run` (capability `host.turn`)
the prompt `"Respond with ONLY a JSON object matching this JSON schema.
Schema: <schema>\nTask: <prompt>"`, extracts the first balanced `{…}` block
from the reply (braces inside strings don't count), and returns it
pretty-printed as the tool result. No parseable JSON → `is_error` carrying
the raw reply. 25s cap (under the host's own 28s host/run deadline).

pi's version was a terminating tool with a fixed output shape; this port
makes the shape a per-call argument and produces it via a nested model turn.

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
