#!/usr/bin/env bash

if [ -z "$SSH_KEY" ]; then
  echo "❌ SSH_KEY is not set"
  exit 1
fi

PROJECT_ROOT=$(git rev-parse --show-toplevel)
# It is too much throughput for most of these providers to deploy to all regions at once
# REGIONS=$(doctl compute region list --format Slug,Available | python -c "
# import sys
# data = sys.stdin.readlines()
# region_dict = {}
# for line in data[1:]:
#     slug, available = line.strip().split()
#     group = slug.rstrip('1234567890')
#     if available == 'true' and group not in region_dict:
#         region_dict[group] = slug
# for region in region_dict.values():
#     print(region)
# ")
REGIONS="nyc1
fra1
sgp1
"
RPC=$(cat $PROJECT_ROOT/.env | grep -v '#' | grep RPC | cut -d '=' -f2 | python -c "import sys; import urllib.parse; print(urllib.parse.urlparse(sys.stdin.read().strip().replace('.', '-').replace('\"', '')).hostname)")

if [ -z "$RPC" ]; then
  echo "❌ RPC is not set in .env file"
  exit 1
fi

echo "🏋️‍♂️ Deploying BenchETH to $RPC ${REGIONS//$'\n'/ } Digital Ocean 🌊"

echo "
➡️ Press enter to continue or Ctrl+C to cancel
"

read

# function to deploy to a region
deploy() {
  region=$1
  droplet="bencheth-$RPC-$region"

  echo "🤠 Deploying to $region $RPC"

  DROPLET_EXISTS=$(doctl compute droplet list --format Name | grep $droplet)

  if [ ! -z "$DROPLET_EXISTS" ]; then
    echo "⏩ Droplet $region IP: $IP already exists, skipping creation"
  else
    echo "⏩ Creating droplet $region $RPC"
    doctl compute droplet create \
      --image docker-20-04 \
      --size g-2vcpu-8gb \
      --region $region \
      --ssh-keys 38154337 \
      --enable-monitoring \
      --wait \
      $droplet
  fi

  IP=$(doctl compute droplet get $droplet --template {{.PublicIPv4}})

  echo "🛜  Droplet $droplet $RPC IP: $IP"

  # wait for droplet to be ready
  output=$(echo quit | telnet "$IP" 22 2>&1)
  while [[ $output != *"Connected"* ]]; do
    echo "⏳ Droplet $droplet $RPC IP: $IP Waiting for droplet to be ready"
    sleep 30
    output=$(echo quit | telnet "$IP" 22 2>&1)
  done

  # rsync deploy
  rsync -avz -e "ssh -i $SSH_KEY -o StrictHostKeyChecking=no" \
    --exclude-from="$PROJECT_ROOT/scripts/deployexclude" \
    $PROJECT_ROOT/ root@$IP:/root/bencheth

  echo "🚀 Deploying $region $RPC to $IP"

  # run deploy script
  ssh -i $SSH_KEY -o StrictHostKeyChecking=no root@$IP <<EOF
    cd /root/bencheth
    docker plugin install grafana/loki-docker-driver:latest --alias loki --grant-all-permissions || true
    sudo ufw allow 9100 # node exporter
    docker compose stop
    docker compose up --build -d
EOF

  echo "✅ Deployed $region $RPC to $IP"
}

for region in $REGIONS; do
  deploy $region &
done

wait
