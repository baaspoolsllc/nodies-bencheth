#!/usr/bin/env bash

CMD=$1

# netstat across each droplet
doctl compute droplet list | grep bencheth | awk '{print $3}' | xargs -I {} ssh -i $SSH_KEY root@{} "$1"
