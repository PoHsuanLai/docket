# Telemetry export (OTLP traces)

The router writes OpenTelemetry GenAI spans: one `execute_tool` per action call and one
`invoke_agent` per model turn. A span carries ids, action names, the effect class, the ruling,
token counts and cost; never a prompt, an argument or a result. Callers of `docket-log` must never
put content on a span; events are not exported, so no event message leaves the machine.

By default nothing leaves the machine: the spans are made and dropped. A daemon built with the
`otlp` feature can send them to a collector on this machine.

## Build

`intentd` (where the router runs) and `companiond` each have an `otlp` feature that turns on
`docket-log/otlp`. It is off by default, and without it `docket-log` does not build the OpenTelemetry crates.

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

Export is on only when one of the two endpoint variables is set. The export is `opentelemetry-otlp`
over HTTP (protobuf) with a blocking client and no TLS, so use an `http://` endpoint; to reach a
remote backend or use TLS, run a local collector and point the daemon at it. If the exporter cannot
be built, one warning goes to the log and export stays off. The `OTEL_*` variables the crates read
beyond the ones above (timeouts, headers) apply as the OpenTelemetry specification says.

Spans are sent in batches by the SDK's batch processor. A batch that cannot be delivered is
dropped. Spans still queued when the daemon stops are sent when `main` drops the guard `init`
returned.

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
