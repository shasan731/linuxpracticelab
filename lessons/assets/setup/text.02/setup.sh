#!/usr/bin/env bash
set -euo pipefail
student_home=/home/student
rm -rf -- "${student_home}/lab" "${student_home}/answers"
mkdir -p "${student_home}/lab" "${student_home}/answers"
for number in $(seq 1 80); do printf "record %02d: routine\n" "${number}"; done > "${student_home}/lab/operations.log"
sed -i "47s/routine/release window at 22:00/" "${student_home}/lab/operations.log"
chown -R student:student "${student_home}/lab" "${student_home}/answers"
