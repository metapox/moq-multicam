#!/bin/bash
# Demo bandwidth throttle script
# Run while recording the browser screen

echo "⏳3s normal - switch cameras now"
sleep 5

echo "🔴 Bandwidth limited to 2000kbps"
docker exec moq-multicam-publisher-1 tc qdisc add dev eth0 root tbf rate 2500kbit burst 64kbit latency 50ms

sleep 30

echo "🟢 Bandwidth restored"
docker exec moq-multicam-publisher-1 tc qdisc del dev eth0 root

echo "⏳ 10s normal - try teleop keys"
sleep 10

echo "✅ Done"
