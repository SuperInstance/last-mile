#!/usr/bin/env bash
# Write the ops entry (the script it calls holds the text).
set -uo pipefail
exec /usr/bin/python3 /home/caseydigennaro/lm-wt/ears/tools/ears/append-ops-log.py
