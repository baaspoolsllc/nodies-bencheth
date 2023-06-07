#!/usr/bin/env bash

DROPLETS=$(doctl compute droplet list | grep bencheth | grep -v ID | awk '{print $1}')

for droplet in $DROPLETS; do
  droplet_name=$(doctl compute droplet get $droplet --format Name --no-header)
  echo "🔥 Destroying droplet $droplet_name"
  echo "Press Ctrl+C to cancel or enter to continue"
  read
  doctl compute droplet delete -f $droplet
done
