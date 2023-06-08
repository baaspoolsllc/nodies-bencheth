#!/usr/bin/env bash

CMD=$1

# netstat across each linode
linode-cli linodes list | grep bencheth | awk '{print $14}' | xargs -n 1 -P 16 -I {} ssh -i $SSH_KEY root@{} "$1"
