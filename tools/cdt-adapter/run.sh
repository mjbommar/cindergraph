#!/bin/sh
set -eu
exec java -cp "/opt/eclipse-cdt-12.6.0/eclipse/plugins/*:$(dirname "$0")" CindergraphCdtAdapter "$@"
