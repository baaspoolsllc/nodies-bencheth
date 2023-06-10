#!/usr/bin/env bash

linodes=$(linode-cli linodes ls | grep bencheth | awk '{print $2}' | grep -vE "^$|id")

linode_names=$(linode-cli linodes ls | grep bencheth | awk '{print $4}' | grep -vE "^$|label")

echo "🔥 Destroying linode ${linode_names}"
echo "Press Ctrl+C to cancel or enter to continue"
read

for linode in $linodes; do
  linode-cli linodes delete $linode
  sleep 2
done
