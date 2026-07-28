#!/usr/bin/env bash
set -euo pipefail
student_home=/home/student
rm -rf -- "${student_home}/lab" "${student_home}/answers"
mkdir -p "${student_home}/lab" "${student_home}/answers"
printf "INFO started\nERROR disk full\nINFO retry\nERROR connection failed\n" > "${student_home}/lab/app.log"
chown -R student:student "${student_home}/lab" "${student_home}/answers"
