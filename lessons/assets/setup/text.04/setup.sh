#!/usr/bin/env bash
set -euo pipefail
student_home=/home/student
rm -rf -- "${student_home}/lab" "${student_home}/answers"
mkdir -p "${student_home}/lab" "${student_home}/answers"
for number in $(seq 1 12); do printf "event-%02d\n" "${number}"; done > "${student_home}/lab/events.log"
chown -R student:student "${student_home}/lab" "${student_home}/answers"
