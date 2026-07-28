#!/usr/bin/env bash
set -euo pipefail
student_home=/home/student
rm -rf -- "${student_home}/lab" "${student_home}/answers"
mkdir -p "${student_home}/lab" "${student_home}/answers"
printf "first line\n" > "${student_home}/answers/report.txt"
chown -R student:student "${student_home}/lab" "${student_home}/answers"
