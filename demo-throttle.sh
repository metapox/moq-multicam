#!/bin/bash
# Demo bandwidth throttle script
# Automatically detects which compose file and service to use.

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
STATUS_FILE="$SCRIPT_DIR/web/demo-status.txt"

# Auto-detect: compose.dev.yml uses publisher-dev, docker-compose.yml uses publisher
if [ -f "$SCRIPT_DIR/compose.dev.yml" ] && docker compose -f "$SCRIPT_DIR/compose.dev.yml" ps --status running 2>/dev/null | grep -q publisher-dev; then
  COMPOSE_FILE="$SCRIPT_DIR/compose.dev.yml"
  SERVICE="publisher-dev"
elif docker compose -f "$SCRIPT_DIR/docker-compose.yml" ps --status running 2>/dev/null | grep -q publisher; then
  COMPOSE_FILE="$SCRIPT_DIR/docker-compose.yml"
  SERVICE="publisher"
else
  echo "❌ No running publisher found. Start with 'docker compose up -d' first."
  exit 1
fi

echo "Using $COMPOSE_FILE → $SERVICE"
echo "" > "$STATUS_FILE"

echo "⏳ 5s normal — switch cameras now"
sleep 5

echo "🔴 Bandwidth limited to 2000kbps"
echo "throttled" > "$STATUS_FILE"
docker compose -f "$COMPOSE_FILE" exec "$SERVICE" \
  sh -c "tc qdisc del dev eth0 root 2>/dev/null; tc qdisc add dev eth0 root tbf rate 2500kbit burst 64kbit latency 50ms"

sleep 30

echo "🟢 Bandwidth restored"
echo "restored" > "$STATUS_FILE"
docker compose -f "$COMPOSE_FILE" exec "$SERVICE" \
  tc qdisc del dev eth0 root

echo "⏳ 10s normal"
sleep 10

echo "✅ Done"
echo "" > "$STATUS_FILE"
