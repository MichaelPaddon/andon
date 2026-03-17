# andon

A signal-routing daemon. Binary (`On`/`Off`) signals
flow from **sources** through optional **nodes** to
**sinks**. Configuration is a single TOML file.

---

## Concepts

| Term   | Role |
|--------|------|
| Source | Produces `On`/`Off` signals and pushes them to named sinks. |
| Sink   | Receives signals from one or more sources and acts on them. |
| Node   | Both source and sink; transforms a signal before forwarding it. |

Sources declare their downstream connections by listing
sink (or node) names in their `sinks` field. Nodes do
the same. The runtime resolves names and wires everything
together at startup.

On startup all alarm sinks (Meross plugs) are turned off,
then all sink connections are established, and only then
do sources begin emitting.  On `SIGINT`/`Ctrl-C` all
alarm sinks are turned off again before the process
exits.

---

## Config file structure

```toml
[[source]]   # zero or more
...

[[node]]     # zero or more
...

[[sink]]     # zero or more
...
```

All three sections are optional.  Every object requires
a `type` field (selects the implementation) and a `name`
field (used by other objects in their `sinks` list).
Names must be unique across all sources, nodes, and
sinks.

---

## Sources

### `type = "url"`

Polls an HTTP endpoint on a schedule and emits `On` when
the check fails, `Off` when it passes.

**Failure conditions (checked in order):**

1. Network or connection error
2. HTTP response status is not 2xx
3. If `pattern` is set: response body does not match

```toml
[[source]]
type     = "url"
name     = "my-service"
url      = "https://example.com/health"
interval = "60s"          # mean polling interval
sinks    = ["my-sink"]

# Optional:
timeout  = "10s"    # per-request timeout (default: 10 s)
stddev   = "5s"     # Gaussian jitter std deviation
pattern  = "\"ok\"" # body must contain a match or signal is On
```

When `stddev` is set each inter-poll sleep is sampled
from `Normal(interval, stddev)`, clamped to zero.

`pattern` is a
[Rust regex](https://docs.rs/regex/latest/regex/#syntax).
Only the presence of a match is tested, not a
full-string match.

---

### `type = "cron"`

Emits `On` at times matching a cron schedule and `Off`
after a fixed duration.

```toml
[[source]]
type     = "cron"
name     = "daily-window"
cron     = "0 0 8 * * *"   # every day at 08:00
duration = "8h"             # stay On for 8 hours
sinks    = ["my-sink"]
```

The `cron` field is a **6-field** expression in local
time:

```
sec  min  hour  day-of-month  month  day-of-week
```

Standard cron wildcards and ranges apply.  If the `On`
period has not ended when the next scheduled time
arrives, the current period runs to completion first.

---

## Nodes

Connect a source to a node the same way you connect it
to a sink: list the node's `name` in the source's
`sinks` field.

All nodes track each input's last known signal
independently (keyed by source name) and only emit when
their output changes.

---

### `type = "not"`

Logic NOT.  Emits `On` when **all** inputs are `Off`;
emits `Off` when **any** input is `On`.

Emits `On` immediately at startup (vacuous case: no
inputs yet → all are off).

```toml
[[node]]
type  = "not"
name  = "invert"
sinks = ["my-sink"]
```

---

### `type = "and"`

Logic AND.  Emits `On` when **all** inputs are `On`;
emits `Off` as soon as **any** input is `Off`.

Does not emit until the first message is received.

```toml
[[node]]
type  = "and"
name  = "all-ok"
sinks = ["my-sink"]
```

---

### `type = "or"`

Logic OR.  Emits `On` when **any** input is `On`; emits
`Off` when **all** inputs are `Off`.

Does not emit until the first message is received.

```toml
[[node]]
type  = "or"
name  = "any-alarm"
sinks = ["my-sink"]
```

---

### `type = "xor"`

Logic XOR.  Emits `On` when an **odd** number of inputs
are `On`; emits `Off` otherwise.

Does not emit until the first message is received.

```toml
[[node]]
type  = "xor"
name  = "parity"
sinks = ["my-sink"]
```

---

### `type = "delay"`

Turn-on delay gate.  Emits `On` only after at least one
input has been continuously `On` for the configured
delay.  Emits `Off` immediately when all inputs go `Off`.
If any input goes `Off` before the delay elapses the
timer is cancelled and `On` is never emitted for that
period.

```toml
[[node]]
type  = "delay"
name  = "debounce"
delay = "30s"
sinks = ["my-sink"]
```

---

## Sinks

### `type = "log"`

Logs every signal change to standard error via the
tracing subscriber.

```toml
[[sink]]
type = "log"
name = "logger"
```

---

### `type = "meross"`

Controls a Meross smart plug via the Meross cloud API.

Turns the plug **on** when any connected source is `On`;
turns it **off** when all connected sources are `Off`.
Only issues a cloud command when the desired state
changes.

Meross sinks are registered as **alarms**: they are
turned off unconditionally at startup and again at
shutdown.

```toml
[[sink]]
type     = "meross"
name     = "alarm-plug"
device   = "Kitchen Plug"       # device name in the Meross app
email    = "user@example.com"
password = "plaintext-password" # see Security note
region   = "eu"                 # optional — default "us"

# Optional:
channel  = 0   # relay channel; 0 = main outlet (default)
```

**`region`** — one of `"eu"`, `"us"`, `"ap"`, or a full
HTTPS base URL for a non-standard endpoint.

**`channel`** — selects the relay on multi-outlet
devices. `0` is the main outlet on single-outlet plugs.

#### Security note

Passwords are stored in plaintext in the config file.
Restrict access with `chmod 600 andon.toml`.

---

## Durations

Duration strings are parsed by
[humantime](https://docs.rs/humantime). Examples:

| String    | Meaning    |
|-----------|------------|
| `"30s"`   | 30 seconds |
| `"1m"`    | 1 minute   |
| `"1m30s"` | 90 seconds |
| `"2h"`    | 2 hours    |

---

## Environment

`RUST_LOG` — controls log verbosity.  The default is
`debug`.  Examples:

```sh
RUST_LOG=info   andon config.toml   # info and above
RUST_LOG=warn   andon config.toml   # warnings and errors only
RUST_LOG=error  andon config.toml   # errors only
```

Filter by module:

```sh
RUST_LOG=andon=info,andon::sources::url=debug andon config.toml
```

---

## Full example

A URL health check feeds a NOT gate during a cron-gated
window.  An AND combines them: the alarm fires only when
the service is down **and** we are inside the on-call
window.  A 30-second delay suppresses brief blips.

```toml
[[source]]
type     = "url"
name     = "api"
url      = "https://api.example.com/health"
interval = "30s"
stddev   = "5s"
timeout  = "5s"
pattern  = "\"status\":\"ok\""
sinks    = ["alarm-gate"]

[[source]]
type     = "cron"
name     = "on-call"
cron     = "0 0 8 * * Mon-Fri"
duration = "10h"
sinks    = ["alarm-gate"]

[[node]]
type  = "and"
name  = "alarm-gate"
sinks = ["debounce"]

[[node]]
type  = "delay"
name  = "debounce"
delay = "30s"
sinks = ["alarm"]

[[sink]]
type     = "meross"
name     = "alarm"
device   = "Alarm Light"
email    = "user@example.com"
password = "s3cr3t"
region   = "eu"
```
