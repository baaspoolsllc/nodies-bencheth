#!/usr/bin/env bash

linodes=$(linode-cli linodes ls | grep bencheth | awk '{print $2}' | grep -vE "^$|id")

for linode in $linodes; do
  linode_name=$(linode-cli linodes view $linode | awk '{print $4}' | grep -vE "^$|label")
  echo "🔥 Destroying linode $linode_name"
  echo "Press Ctrl+C to cancel or enter to continue"
  read
  linode-cli linodes delete $linode
done
