# Authentication

moq-multicam uses JWT-based authentication provided by moq-relay.

## Access Model

| Role | Permissions | JWT Required |
|------|------------|-------------|
| **Publisher** (vehicle cameras) | Publish all paths + subscribe | Yes |
| **Operator** (browser, teleop) | Publish `vehicle/*/control` + subscribe | Yes |
| **Viewer** (browser, watch-only) | Subscribe only | No |

## Quick Setup

```bash
# 1. Generate keys and tokens (run once)
./setup-auth.sh

# 2. Start services (reads MOQ_JWT from .env)
docker compose up
```

The script generates:
- `auth/server.jwk` — ES256 private key (for signing tokens)
- `auth/server.pub.jwk` — ES256 public key (loaded by relay)
- `.env` — `MOQ_JWT` (publisher) and `MOQ_OPERATOR_JWT` (operator)

## Viewer Access

```
# Anonymous viewer (subscribe only, no control)
http://localhost:5173/

# Operator viewer (subscribe + control commands)
http://localhost:5173/?jwt=<MOQ_OPERATOR_JWT>
```

## How It Works

1. **Relay** starts with `--auth-key /auth/server.pub.jwk --auth-public-subscribe ""`
2. **Publisher** connects with `?jwt=TOKEN` in the relay URL — relay verifies the JWT signature and grants publish permissions
3. **Viewer** connects without JWT — relay allows subscribe (anonymous) but denies publish
4. **Operator** connects with JWT — relay allows both subscribe and control publish

## Token Format

JWT claims (ES256 signed):

```json
{
  "put": [""],           // publish path prefixes ("" = all)
  "get": [""],           // subscribe path prefixes ("" = all)
  "exp": 1809325880,     // expiration (unix timestamp)
  "iat": 1777789880      // issued at
}
```

## Manual Token Generation

```bash
cd scripts/gen-token

# Generate key pair
cargo run -- keygen --output-dir ../../auth

# Publisher token (full access, 1 year)
cargo run -- token --key ../../auth/server.jwk --publish ""

# Operator token (control publish only, 1 year)
cargo run -- token --key ../../auth/server.jwk --publish "vehicle/"

# Short-lived token (8 hours)
cargo run -- token --key ../../auth/server.jwk --publish "" --hours 8
```

## Relay Configuration

In `docker-compose.yml` / `compose.dev.yml`:

```yaml
relay:
  command: >
    --auth-key /auth/server.pub.jwk
    --auth-public-subscribe ""
```

- `--auth-key` — path to the public key (verify-only)
- `--auth-public-subscribe ""` — allow anonymous subscribe for all paths

## Security Notes

- Private key (`server.jwk`) has file permissions `0600` and is in `.gitignore`
- `.env` containing tokens is in `.gitignore`
- JWT is passed as a URL query parameter (`?jwt=...`) — this is a moq-relay design choice since QUIC/WebTransport lacks HTTP-style headers at connection time
- Token values are not logged; only the token length is logged on the publisher side
