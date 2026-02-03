#!/usr/bin/env bash
set -euo pipefail

sudo apt-get update
sudo apt-get install -y \
  pkg-config \
  libgtk-3-dev \
  libgdk-pixbuf-2.0-dev \
  libglib2.0-dev \
  libgobject-2.0-dev \
  libwebkit2gtk-4.0-dev
