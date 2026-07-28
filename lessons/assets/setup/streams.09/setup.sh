#!/usr/bin/env bash
set -euo pipefail
student_home=/home/student
rm -rf -- "${student_home}/lab" "${student_home}/answers"
mkdir -p "${student_home}/lab" "${student_home}/answers"
printf "INFO start\nERROR disk\nERROR auth\n" > "${student_home}/lab/app.log"
chown -R student:student "${student_home}/lab" "${student_home}/answers"
