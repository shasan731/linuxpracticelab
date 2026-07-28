#!/usr/bin/env bash
set -euo pipefail
student_home=/home/student
rm -rf -- "${student_home}/lab" "${student_home}/answers"
mkdir -p "${student_home}/lab" "${student_home}/answers"
printf "host=web1\nport=not-a-number\nmode=training\n" > "${student_home}/lab/service.conf"
chown -R student:student "${student_home}/lab" "${student_home}/answers"
