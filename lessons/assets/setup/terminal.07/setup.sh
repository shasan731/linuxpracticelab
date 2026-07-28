#!/usr/bin/env bash
#
# Environment for terminal.07, "Tab Completion".
#
# Builds a tree with a deliberately ambiguous prefix (reports / reporting) so the learner meets
# the two-Tab behaviour rather than only the single-match case, and a path deep enough that
# typing it out by hand is genuinely worse than completing it.

set -Eeuo pipefail

readonly ROOT=/home/student/completion

rm -rf "${ROOT}"
install -d -m 0755 "${ROOT}/reports/quarterly/2024"
install -d -m 0755 "${ROOT}/reporting/templates"
install -d -m 0755 "${ROOT}/archive"

printf 'March figures\n' > "${ROOT}/reports/quarterly/2024/march.txt"
printf 'June figures\n' > "${ROOT}/reports/quarterly/2024/june.txt"
printf 'September figures\n' > "${ROOT}/reports/quarterly/2024/september.txt"
printf 'Quarterly template\n' > "${ROOT}/reporting/templates/quarterly.md"
printf 'Old data\n' > "${ROOT}/archive/2019.txt"

chown -R student:student "${ROOT}"

rm -f /home/student/completed.txt /home/student/ambiguous.txt

exit 0
