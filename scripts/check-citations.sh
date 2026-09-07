#!/usr/bin/env bash
# Shared entry point for CI, the hook, and local corpus validation.
set -euo pipefail
cd "$(dirname "$0")/.."
exec python3 scripts/check-citations.py
