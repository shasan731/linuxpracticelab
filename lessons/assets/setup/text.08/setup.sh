#!/usr/bin/env bash
set -euo pipefail
student_home=/home/student
rm -rf -- "${student_home}/lab" "${student_home}/answers"
mkdir -p "${student_home}/lab" "${student_home}/answers"
printf "environment=staging\n" > "${student_home}/lab/app.conf"
chown -R student:student "${student_home}/lab" "${student_home}/answers"
