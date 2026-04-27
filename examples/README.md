# Examples

| Example | Description | Requires |
|---|---|---|
| `minimal-pubsub` | Minimal moq-lite pub/sub (no video, no relay) | Nothing |
| `multicam-test-source` | 2-camera test source with local subscriber | Nothing |
| `quic-publish` | Publish 2 test cameras to a relay over QUIC | Running relay |
| `quic-subscribe` | Subscribe to a broadcast and log received data | Running relay + publisher |

## Running

```bash
# Minimal (no network)
cargo run -p minimal-pubsub

# Multi-camera test (no network)
cargo run -p multicam-test-source

# With relay (start relay first)
moq-relay --server-bind "[::]:4443" --tls-generate localhost --tls-disable-verify --auth-public "" --web-http-listen "[::]:4443"

# Then in separate terminals:
cargo run -p quic-publish
cargo run -p quic-subscribe
```
