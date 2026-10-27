# Telemetry export (OTLP traces)

The router writes OpenTelemetry GenAI spans: one `execute_tool` per action call and one
`invoke_agent` per model turn. A span carries ids, action names, the effect class, the ruling,
token counts and cost; never a prompt, an argument or a result. Callers of `docket-log` must never
put content on a span, and the exporter sends only the fields a span declared (no events, no event
messages).

By default nothing leaves the machine: the spans are made and dropped. A daemon built with the
`otlp` feature can send them to a collector on this machine.

## Build

`intentd` (where the router runs) and `companiond` each have an `otlp` feature that turns on
`docket-log/otlp`. It is off by default, and without it `docket-log` depends on `tracing` alone.

```sh
cargo build --release -p intentd --features otlp
```

## Turn it on

`init` reads these once, at start, so they are set in the unit, not in a shell:

| Variable | Meaning |
|---|---|
| `OTEL_EXPORTER_OTLP_ENDPOINT` | the collector's base URL; `/v1/traces` is appended |
| `OTEL_EXPORTER_OTLP_TRACES_ENDPOINT` | the full traces URL, used as given; wins over the base one |
| `OTEL_SERVICE_NAME` | the `service.name` resource attribute; the daemon's name when unset |
| `OTEL_SDK_DISABLED` | `true` turns export off whatever else is set |

Only `http://` endpoints are accepted, with `host[:port]` (a missing port is 80) and an optional
path; no query, fragment or user information. An `https://` or malformed endpoint is refused with
one warning in the journal and export stays off. To reach a remote backend or use TLS, run a local
collector and point the daemon at it.

Spans are sent as OTLP/HTTP JSON in batches of up to 512 spans or 5 seconds. A batch that cannot
be delivered is dropped (one warning a minute at most); a full queue drops spans and says how many.
Spans still queued when the daemon stops are lost.

### Example: a local collector on 4318

Run an OpenTelemetry collector or Jaeger on this machine (Jaeger's all-in-one image listens for
OTLP/HTTP on 4318), then add a drop-in for the daemon:

```ini
# ~/.config/systemd/user/intentd.service.d/telemetry.conf
[Service]
Environment=OTEL_EXPORTER_OTLP_ENDPOINT=http://127.0.0.1:4318
Environment=OTEL_SERVICE_NAME=quire-intentd
```

then `systemctl --user daemon-reload && systemctl --user restart intentd`.

The shipped units allow Unix sockets only (`RestrictAddressFamilies=AF_UNIX`) and the sandbox
drop-in gives the daemon a private network (`PrivateNetwork=yes`), so the daemon cannot reach a
collector on the host. Export therefore needs a deliberate, local loosening in the same drop-in:

```ini
[Service]
PrivateNetwork=no
RestrictAddressFamilies=AF_UNIX AF_INET AF_INET6
```

Nothing shipped does this; it is the person's choice, per daemon.
