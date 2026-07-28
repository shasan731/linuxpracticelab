#!/usr/bin/env bash
#
# Environment for orientation.06, "Safe and Dangerous Commands".
#
# Builds a directory holding a deliberate mixture of disposable and irreplaceable files, so the
# learner has to look before deleting. Idempotent, because the same script backs both prepare
# and reset.

set -Eeuo pipefail

readonly HOME_DIR=/home/student
readonly AUDIT="${HOME_DIR}/audit"

rm -rf "${AUDIT}"
install -d -o student -g student -m 0755 "${AUDIT}"

# Files that must survive. The names are deliberately varied so a learner cannot pass by
# matching a single pattern.
printf 'Do not delete this file.\n' > "${AUDIT}/keep-me.txt"
printf 'Q3 revenue: 42\nQ4 revenue: 58\n' > "${AUDIT}/quarterly-report.txt"
printf '# Notes\n\nRemember to check the wildcard first.\n' > "${AUDIT}/notes.md"

# Files that should be removed.
printf 'temporary\n' > "${AUDIT}/scratch.tmp"
printf 'temporary\n' > "${AUDIT}/build-cache.tmp"
printf 'temporary\n' > "${AUDIT}/session-1029.tmp"

chown student:student "${AUDIT}"/*
chmod 0644 "${AUDIT}"/*

# Remove any artefacts from a previous attempt so the checks start from a known state.
rm -f "${HOME_DIR}/audit-plan.txt" "${HOME_DIR}/why-dangerous.txt"

exit 0
