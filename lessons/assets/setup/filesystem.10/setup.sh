#!/usr/bin/env bash
set -euo pipefail
student_home=/home/student
rm -rf -- "${student_home}/lab" "${student_home}/answers"
mkdir -p "${student_home}/lab" "${student_home}/answers"
mkdir -p "${student_home}/lab/maze/start" "${student_home}/lab/maze/project/reports"
printf "navigation complete\n" > "${student_home}/lab/maze/project/.clue"
chown -R student:student "${student_home}/lab" "${student_home}/answers"
