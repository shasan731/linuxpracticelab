#!/usr/bin/env bash
set -euo pipefail
student_home=/home/student
rm -rf -- "${student_home}/lab" "${student_home}/answers"
mkdir -p "${student_home}/lab" "${student_home}/answers"
printf "20\n3\n100\n11\n" > "${student_home}/lab/numbers.txt"
chown -R student:student "${student_home}/lab" "${student_home}/answers"
