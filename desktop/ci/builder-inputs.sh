#!/bin/sh
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# Print what the builder image is made from, as one short hash: the
# Containerfile, the pinned base and BUILDER_REVISION. build-builder.sh puts
# it in the image's label org.tur.builder.inputs; fetch-builder.sh takes the
# image pinned in builder.lock only when its label says the same.
#
#   builder-inputs.sh
set -eu

here=$(dirname "$0")
{
    cat "$here/builder/Containerfile"
    grep -E '^(BASE_IMAGE|BASE_DIGEST|BUILDER_REVISION)=' "$here/builder.lock"
} | sha256sum | cut -c1-16
